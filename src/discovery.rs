//! discovery ctx: live-session scan — sessions/*.json filtered by the `ps` process table.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/discovery.rs
//! Deps:    serde/serde_json; procsrc (the process table); std::fs (called via spawn_blocking)
//! Tested:  inline `#[cfg(test)]` — fixture tests/fixtures/session-file.json + tempdir scan
//!          over a canned `ProcTable` (no process spawn, no fake /proc tree)
//!
//! Key responsibilities:
//! - Parse `~/.claude/sessions/<pid>.json` tolerantly (undocumented internal, assumption A1).
//! - Liveness invariant: session is live iff `ps` reports its pid AND that pid's start time
//!   equals the file's `procStart` string (PID-reuse guard). macOS Claude Code writes
//!   `procStart` as a UTC wall-clock string; `TZ=UTC ps -o lstart=` reproduces it (spec 011).
//! - Carry the session's tty from the same table — the highlight write-target guard.
//!
//! Design constraints:
//! - Read-only over the fleet; never writes into any Claude dir.
//! - Stale files for dead PIDs are EXPECTED — they are counted, never shown live.
//! - The `procStart` token is opaque and Claude-Code-authored: compared, never interpreted.
//! - Parsers stay pure over bytes; `scan` touches only the sessions dir, and takes the process
//!   table as plain data (fetched async by the caller) so it stays sync + spawn-free in tests.

use std::path::Path;

use serde::Deserialize;

use crate::procsrc::{normalize_lstart, ProcFacts, ProcTable};

/// Native coarse status from the session file. Unknown strings preserved (doctor drift signal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeStatus {
    /// Claude is processing.
    Busy,
    /// Waiting at the prompt.
    Idle,
    /// User dropped to shell mode.
    Shell,
    /// Blocked on user input (permission prompt / queued question) — found live 2026-07-10,
    /// the state class the transcript never shows.
    Waiting,
    /// A status string this version of fleetops doesn't know — surfaced, never hidden.
    Other(String),
}

impl From<&str> for NativeStatus {
    fn from(s: &str) -> Self {
        match s {
            "busy" => Self::Busy,
            "idle" => Self::Idle,
            "shell" => Self::Shell,
            "waiting" => Self::Waiting,
            other => Self::Other(other.to_string()),
        }
    }
}

/// One parsed `sessions/<pid>.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFile {
    /// Claude Code process id.
    pub pid: u32,
    /// The session UUID — the aggregate identity.
    pub session_id: String,
    /// Session working directory.
    pub cwd: String,
    /// The process start time Claude Code recorded at launch, as a string (the liveness token).
    /// Opaque and never interpreted — only compared. macOS: UTC wall-clock, `ctime` style.
    pub proc_start: String,
    /// Derived session name (semantic title arrives via telemetry, wave 3).
    pub name: String,
    /// Native coarse status.
    pub status: NativeStatus,
    /// Last update, epoch ms.
    pub updated_at_ms: u64,
    /// Claude Code version that wrote the file (doctor drift signal).
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawSessionFile {
    pid: u32,
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(default)]
    cwd: String,
    #[serde(rename = "procStart")]
    proc_start: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(rename = "updatedAt", default)]
    updated_at_ms: u64,
    #[serde(default)]
    version: Option<String>,
}

/// A live, attributed session — the wave-2 aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    /// Parsed session file.
    pub file: SessionFile,
    /// `CLAUDE_ACCOUNT` from the process environment, if set.
    pub account: Option<String>,
    /// `CMUX_SURFACE_ID` from the process environment — the exact cmux surface this session runs
    /// in, and the jump target. `None` when the session isn't running under cmux (spec 012).
    pub surface_id: Option<String>,
    /// The session's controlling terminal (`/dev/ttys000`), from the process table.
    /// The highlight write target.
    pub pts: Option<String>,
}

/// Scan tallies for the doctor and footer (files seen vs shown).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScanStats {
    /// `*.json` files in the sessions dir.
    pub total_files: usize,
    /// Files that failed to parse (drift signal).
    pub parse_failed: usize,
    /// Parsed files whose PID is dead or reused (expected leftovers).
    pub stale_dead: usize,
    /// Live sessions returned.
    pub live: usize,
    /// The sessions dir itself could not be read — an empty fleet must not look identical
    /// to a failed scan (doctor exits 1 on this; the board footer surfaces it).
    pub dir_unreadable: bool,
    /// The `ps` process table could not be fetched, so liveness is unknowable and EVERY session
    /// reads as dead. Distinct from an empty fleet — the Linux-era `/proc` gap silently shared
    /// one code path with a genuinely dead PID, and the board read as "nothing running".
    pub procs_unavailable: bool,
}

/// Parse one session file. Unknown fields are skipped; missing optional fields defaulted.
pub fn parse_session_file(bytes: &[u8]) -> Option<SessionFile> {
    let raw: RawSessionFile = serde_json::from_slice(bytes).ok()?;
    Some(SessionFile {
        pid: raw.pid,
        session_id: raw.session_id,
        cwd: raw.cwd,
        proc_start: raw.proc_start,
        name: raw.name,
        status: NativeStatus::from(raw.status.as_str()),
        updated_at_ms: raw.updated_at_ms,
        version: raw.version,
    })
}

/// Extract starttime (field 22) from Linux `/proc/<pid>/stat` content.
/// comm (field 2) may contain spaces and parens — fields are counted after the LAST `)`.
///
/// Linux legacy: the Claude lane stopped using this in wave 11 (macOS has no `/proc`); it
/// survives only because `codex.rs` still walks `/proc`. Wave 14 deletes both together.
pub fn starttime_from_stat(stat: &str) -> Option<&str> {
    let after_comm = &stat[stat.rfind(')')? + 1..];
    // after_comm starts at field 3 (state); starttime is field 22 → index 19 here.
    after_comm.split_ascii_whitespace().nth(19)
}

/// Scan `sessions_dir` and filter by liveness against the `ps` process table. Blocking fs work —
/// the sensor calls this inside `spawn_blocking`, having fetched `procs` off the blocking task.
pub fn scan(sessions_dir: &Path, procs: &ProcTable) -> (Vec<LiveSession>, ScanStats) {
    let mut stats = ScanStats::default();
    let mut live = Vec::new();
    let Ok(entries) = std::fs::read_dir(sessions_dir) else {
        stats.dir_unreadable = true;
        return (live, stats);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        stats.total_files += 1;
        let Some(file) = std::fs::read(&path)
            .ok()
            .and_then(|b| parse_session_file(&b))
        else {
            stats.parse_failed += 1;
            continue;
        };
        let Some(facts) = live_facts(procs, file.pid, &file.proc_start) else {
            stats.stale_dead += 1;
            continue;
        };
        live.push(LiveSession {
            file,
            account: facts.account.clone(),
            surface_id: facts.surface_id.clone(),
            pts: facts.tty.clone(),
        });
    }
    stats.live = live.len();
    (live, stats)
}

/// The liveness invariant: `ps` reports `pid`, and that process's start time equals the session
/// file's `proc_start`. Both sides are whitespace-normalized so ps's space-padded day-of-month
/// can never decide liveness. A PID reused since the file was written has a different start
/// time, so it fails here — that is the whole point of the check.
fn live_facts<'t>(procs: &'t ProcTable, pid: u32, proc_start: &str) -> Option<&'t ProcFacts> {
    let facts = procs.get(&pid)?;
    (facts.lstart == normalize_lstart(proc_start)).then_some(facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/session-file.json");

    #[test]
    fn fixture_parses() {
        let f = parse_session_file(FIXTURE).expect("live fixture parses");
        assert_eq!(f.pid, 105_315);
        assert_eq!(f.session_id, "a01d7cea-b33a-4295-aa48-7a058966cdcb");
        assert_eq!(f.cwd, "/Users/user/project-a");
        assert_eq!(
            f.proc_start, "Thu Jul 16 19:10:07 2026",
            "macOS records procStart as a UTC wall-clock string, not Linux clock ticks"
        );
        assert_eq!(f.name, "project-a-fe");
        assert_eq!(f.status, NativeStatus::Shell);
    }

    #[test]
    fn unknown_status_is_preserved_not_dropped() {
        let json = br#"{"pid":1,"sessionId":"s","procStart":"9","status":"pondering"}"#;
        let f = parse_session_file(json).expect("tolerant");
        assert_eq!(f.status, NativeStatus::Other("pondering".to_string()));
        assert_eq!(f.cwd, "", "missing optionals defaulted");
    }

    #[test]
    fn waiting_status_is_first_class() {
        // Found live 2026-07-10 (session 166350) — the input-blocked state.
        let json = br#"{"pid":1,"sessionId":"s","procStart":"9","status":"waiting"}"#;
        let f = parse_session_file(json).expect("parses");
        assert_eq!(f.status, NativeStatus::Waiting);
    }

    #[test]
    fn garbage_and_missing_required_fields_are_none() {
        assert!(parse_session_file(b"not json").is_none());
        assert!(
            parse_session_file(br#"{"pid":1}"#).is_none(),
            "sessionId required"
        );
    }

    #[test]
    fn starttime_survives_parens_and_spaces_in_comm() {
        // After the last ')': state is field 3 (index 0), starttime is field 22 (index 19),
        // so 18 filler fields sit between them.
        let stat = "42 (weird) name)) R 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 START 23";
        assert_eq!(starttime_from_stat(stat), Some("START"));
        assert_eq!(starttime_from_stat("no parens here"), None);
    }

    /// Build a canned process table row — the `ps` output the sensor would have fetched.
    fn proc_row(table: &mut ProcTable, pid: u32, lstart: &str, tty: Option<&str>) {
        table.insert(
            pid,
            ProcFacts {
                lstart: normalize_lstart(lstart),
                tty: tty.map(str::to_string),
                surface_id: None,
                account: None,
            },
        );
    }

    fn session_json(pid: u32, proc_start: &str, status: &str) -> String {
        format!(
            r#"{{"pid":{pid},"sessionId":"sid-{pid}","cwd":"/w","procStart":"{proc_start}","name":"n{pid}","status":"{status}","updatedAt":1}}"#
        )
    }

    #[test]
    fn scan_keeps_live_drops_dead_and_reused_counts_parse_failures() {
        let tmp = std::env::temp_dir().join(format!("fleet-test-{}", std::process::id()));
        let sessions = tmp.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();

        let live_start = "Thu Jul 16 19:10:07 2026";
        let mut procs = ProcTable::new();
        std::fs::write(sessions.join("1.json"), session_json(1, live_start, "busy")).unwrap();
        proc_row(&mut procs, 1, live_start, Some("/dev/ttys000")); // live
        std::fs::write(sessions.join("2.json"), session_json(2, live_start, "idle")).unwrap();
        proc_row(&mut procs, 2, "Thu Jul 16 21:00:00 2026", None); // PID reused: start differs
        std::fs::write(sessions.join("3.json"), session_json(3, live_start, "busy")).unwrap();
        // pid 3: absent from the table — dead
        std::fs::write(sessions.join("4.json"), "garbage").unwrap();
        std::fs::write(sessions.join("README.md"), "not a session").unwrap();

        let (live, stats) = scan(&sessions, &procs);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(stats.total_files, 4);
        assert_eq!(stats.parse_failed, 1);
        assert_eq!(stats.stale_dead, 2, "one reused PID + one dead PID");
        assert_eq!(stats.live, 1);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].file.pid, 1);
        assert_eq!(live[0].pts.as_deref(), Some("/dev/ttys000"));
    }

    #[test]
    fn pid_reuse_is_caught_by_the_start_time_not_the_pid() {
        // THE guard: the pid is alive, but it is a DIFFERENT process than the one that wrote
        // the session file. Dropping this check would "fix" the board by showing lies.
        let tmp = std::env::temp_dir().join(format!("fleet-reuse-{}", std::process::id()));
        let sessions = tmp.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("7.json"),
            session_json(7, "Thu Jul 16 19:10:07 2026", "busy"),
        )
        .unwrap();
        let mut procs = ProcTable::new();
        proc_row(&mut procs, 7, "Thu Jul 16 19:10:08 2026", None); // one second later

        let (live, stats) = scan(&sessions, &procs);
        std::fs::remove_dir_all(&tmp).ok();

        assert!(
            live.is_empty(),
            "a one-second start-time drift is PID reuse"
        );
        assert_eq!(stats.stale_dead, 1);
    }

    #[test]
    fn liveness_survives_a_space_padded_day_of_month() {
        // ps pads a single-digit day ("Jul  6"); Claude Code's padding is unverified. Both sides
        // normalize, so padding must never decide liveness (spec 011 ponytail).
        let tmp = std::env::temp_dir().join(format!("fleet-pad-{}", std::process::id()));
        let sessions = tmp.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("9.json"),
            session_json(9, "Mon Jul 6 19:10:07 2026", "busy"),
        )
        .unwrap();
        let mut procs = ProcTable::new();
        proc_row(&mut procs, 9, "Mon Jul  6 19:10:07 2026", None); // ps's padded form

        let (live, stats) = scan(&sessions, &procs);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(stats.live, 1, "padding is not a start-time difference");
        assert_eq!(live.len(), 1);
    }

    #[test]
    fn session_without_a_tty_is_still_live() {
        let tmp = std::env::temp_dir().join(format!("fleet-env-{}", std::process::id()));
        let sessions = tmp.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let start = "Thu Jul 16 19:10:07 2026";
        std::fs::write(sessions.join("5.json"), session_json(5, start, "busy")).unwrap();
        let mut procs = ProcTable::new();
        proc_row(&mut procs, 5, start, None); // ps reported `??`

        let (live, stats) = scan(&sessions, &procs);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(stats.live, 1, "no tty never drops a live session");
        assert_eq!(live[0].pts, None, "absent → unknown, not error");
        assert_eq!(live[0].account, None, "account lands in wave 12");
    }

    #[test]
    fn scan_of_missing_dir_is_flagged_not_a_silent_empty_fleet() {
        let (live, stats) = scan(Path::new("/nonexistent-fleet-dir"), &ProcTable::new());
        assert!(live.is_empty());
        assert!(stats.dir_unreadable);
        assert_eq!(stats.total_files, 0);
    }

    #[test]
    fn an_empty_process_table_drops_everything() {
        // Why `procs_unavailable` has to exist: a failed `ps` is indistinguishable HERE from a
        // machine with nothing running. The caller must flag it; scan alone cannot tell.
        let tmp = std::env::temp_dir().join(format!("fleet-noprocs-{}", std::process::id()));
        let sessions = tmp.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("1.json"),
            session_json(1, "Thu Jul 16 19:10:07 2026", "busy"),
        )
        .unwrap();

        let (live, stats) = scan(&sessions, &ProcTable::new());
        std::fs::remove_dir_all(&tmp).ok();

        assert!(live.is_empty());
        assert_eq!(stats.stale_dead, 1);
        assert!(
            !stats.procs_unavailable,
            "scan cannot know why the table is empty — the fetcher sets this"
        );
    }
}
