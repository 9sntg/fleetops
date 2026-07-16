//! The process source: pid → start-time + tty, from one `ps` call (macOS `/proc` replacement).
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/procsrc.rs
//! Deps:    runner (the subprocess seam), error
//! Tested:  inline `#[cfg(test)]` — fixture tests/fixtures/ps-table.txt (captured live 2026-07-16)
//!
//! Key responsibilities:
//! - `table_spec`: the one `ps` call — pid, tty and start time for every visible process.
//! - `parse_table`: pure bytes → `ProcTable`, tolerant of malformed lines.
//! - `fetch`: the thin async fetch over the `Runner` seam.
//!
//! Design constraints:
//! - `TZ=UTC` is load-bearing: Claude Code writes `procStart` in UTC, `ps` formats `lstart` in
//!   local time. Without it every session reads as PID-reused (spec 011).
//! - `lstart` is whitespace-normalized on both sides of the liveness compare, so ps's
//!   space-padded day-of-month ("Jul  6") can never decide liveness.
//! - Never a shell string — explicit argv only (rules/rust/subprocess-safety.md).
//! - One call for the whole table, never one `ps` per pid.

use std::collections::HashMap;
use std::time::Duration;

use crate::error::AppResult;
use crate::runner::{CommandSpec, Runner};

/// What fleetops needs to know about one live process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcFacts {
    /// Process start time as `ps` renders it under `TZ=UTC`, whitespace-normalized — the
    /// liveness token compared against the session file's `procStart`.
    pub lstart: String,
    /// The controlling terminal as an absolute path (`/dev/ttys000`); `None` when the process
    /// has none (`ps` prints `??`).
    pub tty: Option<String>,
}

/// pid → facts, for every process visible to this user.
pub type ProcTable = HashMap<u32, ProcFacts>;

/// The process-table call: pid, tty and start time for every process, in one spawn.
pub fn table_spec() -> CommandSpec {
    CommandSpec {
        program: "ps".to_string(),
        args: vec!["-Ao".to_string(), "pid=,tty=,lstart=".to_string()],
        // Claude Code records procStart in UTC; ps would otherwise format in local time.
        env: vec![("TZ".to_string(), "UTC".to_string())],
        timeout: Duration::from_secs(5),
    }
}

/// Collapse runs of whitespace to single spaces, so `"Thu Jul  6 …"` and `"Thu Jul 6 …"` compare
/// equal. Applied to both sides of the liveness check.
pub fn normalize_lstart(raw: &str) -> String {
    raw.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse `ps -Ao pid=,tty=,lstart=` output. Malformed lines are skipped, never fatal — a drifted
/// `ps` must degrade to a smaller table, not a panic.
pub fn parse_table(bytes: &[u8]) -> ProcTable {
    let text = String::from_utf8_lossy(bytes);
    let mut table = ProcTable::new();
    for line in text.lines() {
        let mut fields = line.split_ascii_whitespace();
        let (Some(pid), Some(tty)) = (fields.next(), fields.next()) else {
            continue; // blank or truncated line
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue; // not a pid row
        };
        // Everything after tty is lstart; collecting + joining normalizes the padding.
        let lstart = fields.collect::<Vec<_>>().join(" ");
        if lstart.is_empty() {
            continue; // no start time — unusable for the liveness compare
        }
        table.insert(
            pid,
            ProcFacts {
                lstart,
                tty: parse_tty(tty),
            },
        );
    }
    table
}

/// `ttys000` → `/dev/ttys000`; `??` (no controlling terminal) → `None`.
fn parse_tty(field: &str) -> Option<String> {
    if field == "??" || field == "-" {
        return None;
    }
    Some(format!("/dev/{field}"))
}

/// Fetch the process table. A failure here means liveness is unknowable — the caller must NOT
/// treat it as an empty fleet (spec 011 behaviour 3).
pub async fn fetch(runner: &dyn Runner) -> AppResult<ProcTable> {
    let bytes = runner.run(&table_spec()).await?;
    Ok(parse_table(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/ps-table.txt");

    #[test]
    fn spec_is_explicit_argv_with_utc() {
        let spec = table_spec();
        assert_eq!(spec.program, "ps");
        assert_eq!(spec.args, vec!["-Ao", "pid=,tty=,lstart="]);
        assert!(
            spec.env.contains(&("TZ".to_string(), "UTC".to_string())),
            "TZ=UTC is load-bearing: procStart is UTC, ps formats local"
        );
        assert!(
            !spec.args.iter().any(|a| a.contains(';') || a.contains('|')),
            "explicit argv, never a shell string"
        );
    }

    #[test]
    fn fixture_parses() {
        let table = parse_table(FIXTURE);
        assert_eq!(table.len(), 4, "live fixture: 2 no-tty + 2 session rows");

        let session = table.get(&12696).expect("session pid present");
        assert_eq!(session.lstart, "Thu Jul 16 19:10:07 2026");
        assert_eq!(session.tty.as_deref(), Some("/dev/ttys000"));

        let launchd = table.get(&1).expect("pid 1 present");
        assert_eq!(launchd.tty, None, "?? means no controlling terminal");
        assert_eq!(launchd.lstart, "Thu Jul 16 18:23:29 2026");
    }

    #[test]
    fn lstart_matches_the_session_files_proc_start_verbatim() {
        // The whole port rests on this: sessions/12696.json recorded procStart
        // "Thu Jul 16 19:10:07 2026" (UTC), and TZ=UTC ps reproduces it byte-for-byte.
        let table = parse_table(FIXTURE);
        let observed = &table.get(&12696).expect("pid present").lstart;
        assert_eq!(*observed, normalize_lstart("Thu Jul 16 19:10:07 2026"));
    }

    #[test]
    fn normalize_collapses_the_padded_day() {
        // ps space-pads a single-digit day; whether Claude Code does is unverified, so both
        // sides are normalized and padding can never decide liveness.
        assert_eq!(
            normalize_lstart("Thu Jul  6 19:10:07 2026"),
            normalize_lstart("Thu Jul 6 19:10:07 2026")
        );
        assert_eq!(
            normalize_lstart("  12696 \t x  "),
            "12696 x",
            "leading/trailing/tab runs collapse"
        );
    }

    #[test]
    fn malformed_lines_are_skipped_not_fatal() {
        let input = b"12 ttys001 Thu Jul 16 19:10:07 2026\n\
                      not-a-pid ttys002 Thu Jul 16 19:10:07 2026\n\
                      \n\
                      99 ttys003\n\
                      13 ttys004 Thu Jul 16 19:11:00 2026\n";
        let table = parse_table(input);
        assert_eq!(table.len(), 2, "two good rows survive");
        assert!(table.contains_key(&12) && table.contains_key(&13));
        assert!(
            !table.contains_key(&99),
            "a row with no lstart is unusable for liveness, not a live process"
        );
    }

    #[test]
    fn garbage_is_an_empty_table_not_a_panic() {
        assert!(parse_table(b"").is_empty());
        assert!(parse_table(b"total garbage here").is_empty());
        assert!(parse_table(&[0xff, 0xfe, 0x00]).is_empty());
    }
}
