//! The process source: pid → start-time + tty, from one `ps` call (macOS `/proc` replacement).
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/procsrc.rs
//! Deps:    runner (the subprocess seam), error
//! Tested:  inline `#[cfg(test)]` — fixture tests/fixtures/ps-table.txt (captured live 2026-07-16)
//!
//! Key responsibilities:
//! - `table_spec`: the ONE `ps` call — pid, tty, start time and environment for every process.
//! - `parse_table`: pure bytes → `ProcTable`, tolerant of malformed lines.
//! - `fetch`: the thin async fetch over the `Runner` seam.
//!
//! Design constraints:
//! - `TZ=UTC` is load-bearing: Claude Code writes `procStart` in UTC, `ps` formats `lstart` in
//!   local time. Without it every session reads as PID-reused (spec 011).
//! - `lstart` is whitespace-normalized on both sides of the liveness compare, so ps's
//!   space-padded day-of-month ("Jul  6") can never decide liveness.
//! - `-A` is load-bearing next to `-E`: without it `ps` lists only the caller's terminal, and
//!   every session outside it reads as dead. Guarded by a test on the fixture's shape.
//! - **The environ allowlist is a security boundary.** `-E` exposes every readable process's
//!   environment, which includes secrets — under cmux, `CMUX_SOCKET_CAPABILITY`. ONLY
//!   `CMUX_SURFACE_ID` and `CLAUDE_ACCOUNT` are ever captured; everything else is scanned past
//!   and dropped. Never widen this without re-reading CLAUDE.md's "Never" list.
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
    /// `CMUX_SURFACE_ID` from the process environment — the exact cmux surface this process runs
    /// in, and the whole basis of pane identity (spec 012). `None` outside cmux.
    pub surface_id: Option<String>,
    /// `CLAUDE_ACCOUNT` from the process environment — account attribution. `None` if unset.
    pub account: Option<String>,
}

/// pid → facts, for every process visible to this user.
pub type ProcTable = HashMap<u32, ProcFacts>;

/// The process-table call: pid, tty, start time and environment for every process, in one spawn.
/// `-A` = all processes (without it `ps` lists only the caller's terminal); `-E` = append the
/// environment to the command; `-ww` = never truncate the line.
pub fn table_spec() -> CommandSpec {
    CommandSpec {
        program: "ps".to_string(),
        args: vec![
            "-AEwwo".to_string(),
            "pid=,tty=,lstart=,command=".to_string(),
        ],
        // Claude Code records procStart in UTC; ps would otherwise format in local time.
        env: vec![("TZ".to_string(), "UTC".to_string())],
        timeout: Duration::from_secs(5),
    }
}

/// `lstart` is `ctime`-shaped and always exactly five whitespace-separated tokens:
/// `Thu Jul 16 19:10:07 2026`. Everything after them is the command + its environment.
const LSTART_FIELDS: usize = 5;

/// Collapse runs of whitespace to single spaces, so `"Thu Jul  6 …"` and `"Thu Jul 6 …"` compare
/// equal. Applied to both sides of the liveness check.
pub fn normalize_lstart(raw: &str) -> String {
    raw.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse `ps -AEwwo pid=,tty=,lstart=,command=` output. Malformed lines are skipped, never fatal
/// — a drifted `ps` must degrade to a smaller table, not a panic.
pub fn parse_table(bytes: &[u8]) -> ProcTable {
    let text = String::from_utf8_lossy(bytes);
    let mut table = ProcTable::new();
    for line in text.lines() {
        if let Some((pid, facts)) = parse_row(line) {
            table.insert(pid, facts);
        }
    }
    table
}

fn parse_row(line: &str) -> Option<(u32, ProcFacts)> {
    let mut fields = line.split_ascii_whitespace();
    let pid: u32 = fields.next()?.parse().ok()?;
    let tty = fields.next()?;
    // Exactly five tokens of lstart; re-joining with single spaces normalizes ps's padding.
    let lstart: Vec<&str> = fields.by_ref().take(LSTART_FIELDS).collect();
    if lstart.len() < LSTART_FIELDS {
        return None; // no usable start time — cannot establish liveness for this row
    }
    // The remainder is argv + environ. Scan it for ONLY the two allowlisted variables and drop
    // the rest — it contains secrets (see the module header).
    let mut surface_id = None;
    let mut account = None;
    for token in fields {
        if let Some(v) = token.strip_prefix("CMUX_SURFACE_ID=") {
            surface_id = Some(v.to_string());
        } else if let Some(v) = token.strip_prefix("CLAUDE_ACCOUNT=") {
            account = Some(v.to_string());
        }
    }
    Some((
        pid,
        ProcFacts {
            lstart: lstart.join(" "),
            tty: parse_tty(tty),
            surface_id,
            account,
        },
    ))
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
    fn spec_is_explicit_argv_with_utc_and_all_processes() {
        let spec = table_spec();
        assert_eq!(spec.program, "ps");
        assert_eq!(spec.args, vec!["-AEwwo", "pid=,tty=,lstart=,command="]);
        assert!(
            spec.args[0].contains('A'),
            "-A is load-bearing: without it ps lists only the caller's terminal and every \
             session elsewhere reads as dead"
        );
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
        assert_eq!(table.len(), 2, "live fixture: launchd + one cmux session");

        let session = table.get(&23223).expect("session pid present");
        assert_eq!(session.lstart, "Thu Jul 16 19:39:35 2026");
        assert_eq!(session.tty.as_deref(), Some("/dev/ttys005"));
        assert_eq!(
            session.surface_id.as_deref(),
            Some("02EC2459-2B77-4FA8-A51C-452E80CA19F8"),
            "the cmux surface id is the exact pane identity"
        );

        let launchd = table.get(&1).expect("pid 1 present");
        assert_eq!(launchd.tty, None, "?? means no controlling terminal");
        assert_eq!(launchd.lstart, "Thu Jul 16 18:23:29 2026");
        assert_eq!(launchd.surface_id, None, "not under cmux");
    }

    #[test]
    fn the_environ_allowlist_drops_secrets() {
        // SECURITY BOUNDARY. `ps -E` hands us every readable process's environment, which under
        // cmux includes CMUX_SOCKET_CAPABILITY — an auth token. The fixture deliberately carries
        // one. Nothing outside the allowlist may ever be captured.
        let table = parse_table(FIXTURE);
        let session = table.get(&23223).expect("session pid present");
        let captured = format!("{session:?}");
        assert!(
            !captured.contains("CMUX_SOCKET_CAPABILITY") && !captured.contains("REDACTED-SECRET"),
            "the socket capability must never be captured into ProcFacts: {captured}"
        );
        assert!(
            !captured.contains("preferredNotifChannel"),
            "argv (which carries the whole --settings blob) must not be captured either"
        );
    }

    #[test]
    fn lstart_matches_the_session_files_proc_start_verbatim() {
        // The whole port rests on this: the session file recorded procStart in UTC, and
        // TZ=UTC ps reproduces it byte-for-byte.
        let table = parse_table(FIXTURE);
        let observed = &table.get(&23223).expect("pid present").lstart;
        assert_eq!(*observed, normalize_lstart("Thu Jul 16 19:39:35 2026"));
    }

    #[test]
    fn a_command_line_is_never_mistaken_for_a_start_time() {
        // lstart is exactly five tokens; the command that follows must not leak into it.
        let table = parse_table(b"42 ttys001 Thu Jul 16 19:10:07 2026 /bin/zsh -l FOO=bar\n");
        let facts = table.get(&42).expect("row parses");
        assert_eq!(facts.lstart, "Thu Jul 16 19:10:07 2026");
        assert_eq!(facts.surface_id, None);
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
            "a row with a truncated lstart is unusable for liveness, not a live process"
        );
    }

    #[tokio::test]
    async fn fetch_parses_the_table_with_no_process_spawn() {
        use crate::runner::CannedRunner;
        let runner = CannedRunner::new(FIXTURE.to_vec());
        let table = fetch(&runner).await.expect("canned bytes parse");
        assert_eq!(table.len(), 2);
        let spec = runner.last_spec().expect("one call");
        assert_eq!(spec.program, "ps");
        assert_eq!(spec.env, vec![("TZ".to_string(), "UTC".to_string())]);
    }

    #[tokio::test]
    async fn fetch_propagates_failure_rather_than_returning_an_empty_table() {
        use crate::error::AppError;
        use crate::runner::CannedRunner;
        // An empty table and a failed ps are indistinguishable downstream, and one of them means
        // "every session is dead". `fetch` must not flatten the error away (spec 011).
        let runner = CannedRunner::new_seq(vec![Err(AppError::Subprocess {
            program: "ps".to_string(),
            message: "exit 1".to_string(),
        })]);
        assert!(fetch(&runner).await.is_err());
    }

    #[test]
    fn account_is_extracted_when_present() {
        let table =
            parse_table(b"7 ttys001 Thu Jul 16 19:10:07 2026 claude CLAUDE_ACCOUNT=alpha\n");
        assert_eq!(
            table.get(&7).expect("row").account.as_deref(),
            Some("alpha")
        );
    }

    #[test]
    fn garbage_is_an_empty_table_not_a_panic() {
        assert!(parse_table(b"").is_empty());
        assert!(parse_table(b"total garbage here").is_empty());
        assert!(parse_table(&[0xff, 0xfe, 0x00]).is_empty());
    }
}
