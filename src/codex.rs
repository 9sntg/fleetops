//! codex ctx: Codex CLI TUI sessions on the board — recognize the process, join its rollout,
//! fold status/tokens/name from the tail. All pure except `scan` (spec 008).
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/codex.rs
//! Deps:    serde/serde_json (rollout JSON); std::fs (via `scan`, called by the sensor's
//!          spawn_blocking); board (SessionRow, match_surface); discovery
//!          (starttime_from_stat); fold (Status, STALL_AFTER_SECS); cmux (Surface)
//! Tested:  inline `#[cfg(test)]` — synthetic rollout JSONL lines + tempdir fake `/proc` +
//!          `~/.codex/sessions` tree (house pattern, see discovery.rs/telemetry.rs)
//!
//! Key responsibilities:
//! - Recognize a Codex TUI process: `comm == "codex"`, argv0-only cmdline, `fd/1 -> /dev/pts/*`
//!   (`is_codex_tui`) — the node shim (`comm == "node"`) and `codex exec`/`--version` are
//!   skipped for free (comm mismatch / extra argv).
//! - Parse a rollout's `session_meta` line 0 (`parse_session_meta`) and fold its tail
//!   (`fold_rollout_tail`) into status/tokens/ctx%/name per the spec 008 status table.
//! - Join each live process to its newest same-cwd rollout, without sqlite (v1): a liveness
//!   guard rejects a rollout mtime older than the process's own start minus a slack window; two
//!   processes sharing a cwd never join (`join_rollouts` — never guess, house rule).
//! - `scan`: the one fs-touching entry point, mirroring `discovery::scan`'s shape — walks
//!   `proc_root` for live Codex TUIs and `codex_root/sessions/**/rollout-*.jsonl` for
//!   candidates, joins them, and assembles `SessionRow`s (matched via `board::match_surface`).
//!
//! Design constraints:
//! - Read-only over `~/.codex`; never writes.
//! - Parsers stay pure over already-read bytes/facts; only `scan` touches the fs (and its own
//!   `SystemTime::now()` for rollout age — the one impure edge, kept inside `scan`).
//! - No sqlite dependency this wave (recon: `~/.codex/logs_2.sqlite` would join exactly — the
//!   recorded upgrade trigger if cwd-join ambiguity bites in practice, not v1).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use crate::board::{self, SessionRow};
use crate::cmux::Surface;
use crate::discovery;
use crate::fold::{self, Status};

/// `session_meta` line 0 of a rollout — tolerant, unknown fields skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMeta {
    /// The rollout's session uuid.
    pub id: String,
    /// The Codex process's cwd at session start — the join key.
    pub cwd: String,
}

#[derive(Debug, Deserialize)]
struct RawSessionMetaPayload {
    id: String,
    cwd: String,
}

#[derive(Debug, Deserialize)]
struct RawSessionMeta {
    #[serde(rename = "type")]
    kind: String,
    payload: RawSessionMetaPayload,
}

/// Parse a rollout's line 0 — tolerant `serde_json`: unknown top-level fields (originator,
/// cli_version, source, timestamp) are skipped; only `type == "session_meta"` plus
/// `payload.{id,cwd}` are extracted.
pub fn parse_session_meta(bytes: &[u8]) -> Option<SessionMeta> {
    let raw: RawSessionMeta = serde_json::from_slice(bytes).ok()?;
    if raw.kind != "session_meta" {
        return None;
    }
    Some(SessionMeta {
        id: raw.payload.id,
        cwd: raw.payload.cwd,
    })
}

/// Facts folded from a rollout tail (spec 008 status table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutFacts {
    /// Folded status.
    pub status: Status,
    /// Total token usage from the last `token_count` line.
    pub tokens: Option<u64>,
    /// `total * 100 / model_context_window` from that same line.
    pub ctx_pct: Option<u8>,
    /// Last `user_message` text's first line, truncated to 60 chars — the semantic name.
    pub name: Option<String>,
}

/// Fold a rollout tail (last `TAIL_BYTES`) into status/tokens/ctx%/name. `age_secs` is the rollout
/// file's mtime age — Working vs Stalled hinges on it, exactly like `fold::STALL_AFTER_SECS`.
/// Tolerant: garbage/unknown lines are skipped, never fatal (spec 008 status table).
pub fn fold_rollout_tail(bytes: &[u8], age_secs: Option<u64>) -> RolloutFacts {
    // The last-seen signal wins — lines are processed in file order, so a later line always
    // overrides an earlier one (e.g. a `task_complete` after an approval request resolves it).
    #[derive(Clone, Copy)]
    enum Signal {
        Complete,
        Activity,
        NeedsAnswer,
    }

    let mut signal: Option<Signal> = None;
    let mut tokens: Option<u64> = None;
    let mut ctx_pct: Option<u8> = None;
    let mut name: Option<String> = None;

    for line in bytes.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue; // garbage / truncated line — skip, never fail (tolerant-parser invariant)
        };
        match value.get("type").and_then(Value::as_str) {
            // Streaming model output — its own top-level envelope, ground-truthed against a
            // live rollout (any subtype counts as activity, the turn is live).
            Some("response_item") => signal = Some(Signal::Activity),
            Some("event_msg") => {
                let Some(kind) = value.pointer("/payload/type").and_then(Value::as_str) else {
                    continue;
                };
                match kind {
                    "task_complete" => signal = Some(Signal::Complete),
                    "task_started" | "token_count" => signal = Some(Signal::Activity),
                    "exec_approval_request"
                    | "apply_patch_approval_request"
                    | "elicitation_request"
                    | "request_user_input" => signal = Some(Signal::NeedsAnswer),
                    "user_message" => {
                        if let Some(text) =
                            value.pointer("/payload/message").and_then(Value::as_str)
                        {
                            // First line only — an embedded newline (pasted code, a multi-line
                            // prompt) must never reach the name (spec 008).
                            name = text
                                .lines()
                                .next()
                                .map(|line| line.chars().take(60).collect());
                        }
                    }
                    _ => {}
                }
                if kind == "token_count" {
                    if let Some(total) = value
                        .pointer("/payload/info/total_token_usage/total_tokens")
                        .and_then(Value::as_u64)
                    {
                        tokens = Some(total);
                        ctx_pct = value
                            .pointer("/payload/info/model_context_window")
                            .and_then(Value::as_u64)
                            .filter(|&window| window > 0)
                            .map(|window| {
                                let pct = total.saturating_mul(100) / window;
                                u8::try_from(pct.min(u64::from(u8::MAX))).unwrap_or(u8::MAX)
                            });
                    }
                }
            }
            _ => {} // unknown envelope type: skip (tolerant by design)
        }
    }

    let status = match signal {
        None | Some(Signal::Complete) => Status::Idle,
        Some(Signal::NeedsAnswer) => Status::NeedsAnswer,
        Some(Signal::Activity) => match age_secs {
            Some(age) if age > fold::STALL_AFTER_SECS => Status::Stalled,
            _ => Status::Working,
        },
    };

    RolloutFacts {
        status,
        tokens,
        ctx_pct,
        name,
    }
}

/// A Codex TUI process: `comm == "codex"` AND argv0-only cmdline AND `fd/1 -> /dev/pts/*`. The
/// node shim (`comm == "node"`) and `codex exec`/`codex --version` are skipped for free (comm
/// mismatch / extra argv).
pub fn is_codex_tui(comm: &str, cmdline: &[u8], fd1_target: Option<&str>) -> bool {
    let argv0_only = cmdline
        .split(|&b| b == 0)
        .filter(|arg| !arg.is_empty())
        .count()
        == 1;
    comm.trim_end() == "codex"
        && argv0_only
        && fd1_target.is_some_and(|t| t.starts_with("/dev/pts/"))
}

/// One live Codex process's already-read join facts (spec 008 discovery).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexProc {
    /// `/proc/<pid>/cwd` readlink target.
    pub cwd: String,
    /// Wallclock seconds the process started: `btime + starttime/HZ` (the join liveness guard
    /// baseline; `HZ` is hardcoded at 100 for this WSL2 kernel — a wrong value only
    /// loosens the guard, degrading to newest-per-cwd).
    pub start_wallclock_secs: u64,
}

/// One rollout candidate: its parsed `session_meta` plus the file's mtime (join input).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutCandidate {
    /// The rollout's session uuid.
    pub session_id: String,
    /// `session_meta.cwd` — the join key.
    pub cwd: String,
    /// The rollout file's mtime, epoch seconds.
    pub mtime_secs: u64,
}

/// Liveness-join slack (spec 008): a rollout can't be older than the process's own start minus
/// this many seconds.
const JOIN_SLACK_SECS: u64 = 600;

/// Join each process (same order in, same order out) to its newest same-cwd rollout candidate
/// whose mtime isn't older than the process's own start minus the liveness slack (spec 008: 600
/// s). Two processes sharing a cwd never join — never guess which rollout is whose (house
/// rule).
pub fn join_rollouts<'a>(
    procs: &[CodexProc],
    rollouts: &'a [RolloutCandidate],
) -> Vec<Option<&'a RolloutCandidate>> {
    procs
        .iter()
        .map(|proc| {
            let shared_cwd = procs.iter().filter(|p| p.cwd == proc.cwd).count() > 1;
            if shared_cwd {
                return None;
            }
            let min_mtime = proc.start_wallclock_secs.saturating_sub(JOIN_SLACK_SECS);
            rollouts
                .iter()
                .filter(|r| r.cwd == proc.cwd && r.mtime_secs >= min_mtime)
                .max_by_key(|r| r.mtime_secs)
        })
        .collect()
}

/// Cap on rollout files scanned per sweep — bounds cost as `~/.codex/sessions` accumulates.
const MAX_ROLLOUTS: usize = 300;
/// Rollout tail read window — matches `telemetry`'s transcript tail window (256 KiB). A single
/// codex turn can stream well over 64 KiB of `response_item` output, which would push the last
/// `user_message` line out of a smaller window and cost the row its name.
const TAIL_BYTES: u64 = 256 * 1024;
/// WSL2 clock ticks/sec (HZ) — hardcoded per recon; a wrong value only loosens the join guard
/// (degrading to newest-per-cwd), never rejects a live process.
const HZ: u64 = 100;

/// One live Codex TUI process's already-read facts (scan-internal; `CodexProc` is the pure join
/// input derived from this).
struct ProcInfo {
    pid: u32,
    cwd: String,
    pts: Option<String>,
    surface_id: Option<String>,
    start_wallclock_secs: u64,
}

/// Scan `codex_root` for rollouts and `proc_root` for live Codex TUI processes, join them, and
/// return assembled `SessionRow`s — matched against `surfaces` via `board::match_surface`
/// (env pane id only). Blocking fs work — the sensor calls this inside `spawn_blocking`, same
/// pattern as `discovery::scan`.
pub fn scan(codex_root: &Path, proc_root: &Path, surfaces: &[Surface]) -> Vec<SessionRow> {
    let proc_infos = scan_procs(proc_root);
    let (candidates, paths_by_id) = scan_rollouts(codex_root);
    let join_procs: Vec<CodexProc> = proc_infos
        .iter()
        .map(|p| CodexProc {
            cwd: p.cwd.clone(),
            start_wallclock_secs: p.start_wallclock_secs,
        })
        .collect();
    let joined = join_rollouts(&join_procs, &candidates);
    // The one impure edge (spec 008): rollout age is computed against wallclock now, here only.
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    proc_infos
        .iter()
        .zip(joined)
        .map(|(proc, matched)| {
            let shares_cwd = proc_infos.iter().filter(|p| p.cwd == proc.cwd).count() > 1;
            build_row(now_secs, proc, matched, &paths_by_id, shares_cwd, surfaces)
        })
        .collect()
}

/// Walk `proc_root` for live Codex TUI processes (comm/cmdline/fd1 gate via `is_codex_tui`).
fn scan_procs(proc_root: &Path) -> Vec<ProcInfo> {
    let btime = read_btime(proc_root);
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            read_proc_info(proc_root, pid, btime)
        })
        .collect()
}

/// `btime` (boot time, epoch secs) from `<proc_root>/stat` — missing/unreadable degrades to 0,
/// which only loosens the join liveness guard (spec 008), never rejects a live process.
fn read_btime(proc_root: &Path) -> u64 {
    std::fs::read_to_string(proc_root.join("stat"))
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find_map(|l| l.strip_prefix("btime "))
                .and_then(|v| v.trim().parse().ok())
        })
        .unwrap_or(0)
}

/// Read one `/proc/<pid>`'s facts; `None` unless it passes the `is_codex_tui` gate.
fn read_proc_info(proc_root: &Path, pid: u32, btime: u64) -> Option<ProcInfo> {
    let dir = proc_root.join(pid.to_string());
    let comm = std::fs::read_to_string(dir.join("comm")).ok()?;
    let cmdline = std::fs::read(dir.join("cmdline")).ok()?;
    let fd1_target = std::fs::read_link(dir.join("fd").join("1"))
        .ok()
        .and_then(|t| t.to_str().map(str::to_string));
    if !is_codex_tui(&comm, &cmdline, fd1_target.as_deref()) {
        return None;
    }
    let cwd = std::fs::read_link(dir.join("cwd"))
        .ok()
        .and_then(|t| t.to_str().map(str::to_string))?;
    let stat = std::fs::read_to_string(dir.join("stat")).ok()?;
    let starttime_ticks: u64 = discovery::starttime_from_stat(&stat)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Some(ProcInfo {
        pid,
        cwd,
        pts: fd1_target,
        surface_id: None, // wave 14: the macOS codex lane reads CMUX_SURFACE_ID from procsrc
        start_wallclock_secs: btime + starttime_ticks / HZ,
    })
}

/// Walk `codex_root/sessions/*/*/*/rollout-*.jsonl`, newest-first by MTIME, capped, parsed into
/// join candidates + a session-id -> path index (for the tail read once joined).
///
/// Sorted by mtime, not filename (which encodes session START time, not last-write): a
/// long-running session's rollout is appended to continuously, so its mtime stays fresh even
/// as its filename ages — sorting by filename would eventually truncate it out of the cap and
/// it could never join again. Mtime-descending is the only ordering under which an
/// actively-appended rollout never ages out.
///
/// ponytail: no cache — every file is re-stat'd every sweep. Fine at the current cap (300);
/// mirror `TailCache`'s (size, mtime) keying here if a growing `~/.codex/sessions` makes this
/// sweep measurably slow.
fn scan_rollouts(codex_root: &Path) -> (Vec<RolloutCandidate>, HashMap<String, PathBuf>) {
    let mut paths_only = Vec::new();
    collect_rollout_files(&codex_root.join("sessions"), &mut paths_only);
    let mut files: Vec<(PathBuf, u64)> = paths_only
        .into_iter()
        .filter_map(|p| mtime_epoch_secs(&p).map(|mtime| (p, mtime)))
        .collect();
    files.sort_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));
    files.truncate(MAX_ROLLOUTS);

    let mut candidates = Vec::new();
    let mut paths = HashMap::new();
    for (path, mtime_secs) in files {
        let Some(meta) = read_session_meta_line(&path) else {
            continue;
        };
        paths.insert(meta.id.clone(), path);
        candidates.push(RolloutCandidate {
            session_id: meta.id,
            cwd: meta.cwd,
            mtime_secs,
        });
    }
    (candidates, paths)
}

/// Collect every `rollout-*.jsonl` three directory levels under `sessions_dir` (YYYY/MM/DD).
fn collect_rollout_files(sessions_dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(years) = std::fs::read_dir(sessions_dir) else {
        return;
    };
    for year in years.flatten() {
        let Ok(months) = std::fs::read_dir(year.path()) else {
            continue;
        };
        for month in months.flatten() {
            let Ok(days) = std::fs::read_dir(month.path()) else {
                continue;
            };
            for day in days.flatten() {
                let Ok(entries) = std::fs::read_dir(day.path()) else {
                    continue;
                };
                out.extend(entries.flatten().map(|e| e.path()).filter(|p| {
                    p.extension().is_some_and(|ext| ext == "jsonl")
                        && p.file_stem()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("rollout-"))
                }));
            }
        }
    }
}

/// The rollout file's mtime, epoch seconds — `None` if the file vanished mid-scan.
fn mtime_epoch_secs(path: &Path) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Read and parse just line 0 of a rollout — the join candidate needs nothing more.
fn read_session_meta_line(path: &Path) -> Option<SessionMeta> {
    let file = std::fs::File::open(path).ok()?;
    let mut line = String::new();
    BufReader::new(file).read_line(&mut line).ok()?;
    parse_session_meta(line.as_bytes())
}

/// Read the last `TAIL_BYTES` of a rollout file — same tail-read pattern as `telemetry`.
fn read_tail(path: &Path) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let offset = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Build one `SessionRow` from a live process + its (possibly absent) joined rollout.
/// `shares_cwd` distinguishes the two unjoined placeholder names (spec 008): a genuinely
/// promptless TUI vs. one whose cwd collided with a sibling process (never guessed).
fn build_row(
    now_secs: u64,
    proc: &ProcInfo,
    matched: Option<&RolloutCandidate>,
    paths_by_id: &HashMap<String, PathBuf>,
    shares_cwd: bool,
    surfaces: &[Surface],
) -> SessionRow {
    let pane = board::match_surface(proc.surface_id.as_deref(), surfaces);
    // The highlight write-target guard, same as the Claude lane (wave 6, spec 006): a process is
    // only ever highlightable when it renders in a wezterm pane.
    let pts = if proc.surface_id.is_some() {
        proc.pts.clone()
    } else {
        None
    };

    let Some(candidate) = matched else {
        let name = if shares_cwd {
            "codex — session ambiguous"
        } else {
            "codex — no prompt yet"
        };
        return SessionRow {
            session_id: format!("codex-pid-{}", proc.pid),
            name: name.to_string(),
            account: Some("codex".to_string()),
            status: Status::Idle,
            cwd: proc.cwd.clone(),
            context_tokens: None,
            ctx_pct: None,
            secs_since_append: None,
            pane,
            pts,
        };
    };

    let tail = paths_by_id
        .get(&candidate.session_id)
        .and_then(|p| read_tail(p))
        .unwrap_or_default();
    let age_secs = now_secs.checked_sub(candidate.mtime_secs);
    let facts = fold_rollout_tail(&tail, age_secs);

    SessionRow {
        session_id: candidate.session_id.clone(),
        // A row that joined a rollout but whose tail hasn't seen a `user_message` yet is
        // mid-conversation (the rollout exists, the process is live) — "no prompt yet" is
        // reserved for the unjoined placeholder above; this must never claim there's no prompt.
        name: facts.name.unwrap_or_else(|| "codex (untitled)".to_string()),
        account: Some("codex".to_string()),
        status: facts.status,
        cwd: proc.cwd.clone(),
        context_tokens: facts.tokens,
        ctx_pct: facts.ctx_pct,
        secs_since_append: age_secs,
        pane,
        pts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fold::STALL_AFTER_SECS;

    // --- is_codex_tui ---

    #[test]
    fn is_codex_tui_table() {
        let cases: &[(&str, &[u8], Option<&str>, bool)] = &[
            // the interactive TUI: comm codex, argv0 only, real pts
            ("codex\n", b"codex\0", Some("/dev/pts/4"), true),
            // `codex exec <prompt>` — extra argv, transient, must be skipped
            ("codex\n", b"codex\0exec\0", Some("/dev/pts/4"), false),
            // `codex --version` — extra argv, transient, must be skipped
            ("codex\n", b"codex\0--version\0", Some("/dev/pts/4"), false),
            // the node shim wrapping codex — comm mismatch, skipped for free
            ("node\n", b"codex\0", Some("/dev/pts/4"), false),
            // fd/1 not a pty (e.g. redirected to a file) — never a TUI target
            ("codex\n", b"codex\0", Some("/dev/null"), false),
            ("codex\n", b"codex\0", None, false),
        ];
        for (comm, cmdline, fd1, want) in cases {
            assert_eq!(
                is_codex_tui(comm, cmdline, *fd1),
                *want,
                "comm={comm:?} cmdline={cmdline:?} fd1={fd1:?}"
            );
        }
    }

    // --- parse_session_meta ---

    #[test]
    fn parse_session_meta_fixture_line() {
        // Captured shape (spec 008 recon): unknown top-level fields (originator/cli_version/
        // source/timestamp) must be tolerated.
        let line = br#"{"timestamp":"2026-07-10T12:00:00Z","type":"session_meta","payload":{"id":"7c9e6679-7425-40de-944b-e07fc1f90ae7","cwd":"/home/user/x","originator":"codex-tui","cli_version":"0.144.1","source":"cli"}}"#;
        let want = SessionMeta {
            id: "7c9e6679-7425-40de-944b-e07fc1f90ae7".to_string(),
            cwd: "/home/user/x".to_string(),
        };
        assert_eq!(parse_session_meta(line), Some(want));
    }

    #[test]
    fn parse_session_meta_rejects_wrong_type_and_garbage() {
        assert!(parse_session_meta(b"not json").is_none());
        assert!(parse_session_meta(br#"{"type":"task_started"}"#).is_none());
    }

    // --- fold_rollout_tail ---

    // Real envelope (ground-truthed against a live rollout 2026-07-10): event lines are
    // `type: "event_msg"` with the discriminator nested at `payload.type`.
    fn event_line(event_type: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-07-10T00:00:00Z","type":"event_msg","payload":{{"type":"{event_type}"}}}}"#
        )
    }

    // Streaming model output: top-level `type: "response_item"`, its own subtype in payload.
    fn response_item_line(item_type: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-07-10T00:00:00Z","type":"response_item","payload":{{"type":"{item_type}","role":"assistant"}}}}"#
        )
    }

    // Ground truth: usage lives under `payload.info`, the total under `total_tokens`.
    fn token_count_line(total: u64, window: u64) -> String {
        format!(
            r#"{{"timestamp":"2026-07-10T00:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":1,"cached_input_tokens":2,"output_tokens":3,"reasoning_output_tokens":4,"total_tokens":{total}}},"model_context_window":{window}}},"rate_limits":null}}}}"#
        )
    }

    // Ground truth: the prompt text field is `message` (not `text`).
    fn user_message_line(text: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-07-10T00:00:00Z","type":"event_msg","payload":{{"type":"user_message","message":"{text}","images":[],"local_images":[]}}}}"#
        )
    }

    #[test]
    fn fold_last_event_task_complete_is_idle() {
        let tail = [event_line("task_started"), event_line("task_complete")].join("\n");
        assert_eq!(
            fold_rollout_tail(tail.as_bytes(), Some(5)).status,
            Status::Idle
        );
    }

    #[test]
    fn fold_task_started_after_complete_within_stall_window_is_working() {
        let tail = [event_line("task_complete"), event_line("task_started")].join("\n");
        assert_eq!(
            fold_rollout_tail(tail.as_bytes(), Some(10)).status,
            Status::Working,
            "fresh activity after the last task_complete"
        );
    }

    #[test]
    fn fold_response_item_after_complete_is_activity_too() {
        // Streaming output (`response_item`) counts as activity, same as task_started.
        let tail = [event_line("task_complete"), response_item_line("message")].join("\n");
        assert_eq!(
            fold_rollout_tail(tail.as_bytes(), Some(10)).status,
            Status::Working,
            "model is streaming — the turn is live even without a task_started tail"
        );
    }

    #[test]
    fn fold_task_started_after_complete_past_stall_window_is_stalled() {
        let tail = [event_line("task_complete"), event_line("task_started")].join("\n");
        assert_eq!(
            fold_rollout_tail(tail.as_bytes(), Some(STALL_AFTER_SECS + 1)).status,
            Status::Stalled,
            "301s of silence after task_started"
        );
    }

    #[test]
    fn fold_approval_request_family_with_no_later_complete_is_needs_answer() {
        for kind in [
            "exec_approval_request",
            "apply_patch_approval_request",
            "elicitation_request",
            "request_user_input",
        ] {
            let tail = [event_line("task_started"), event_line(kind)].join("\n");
            assert_eq!(
                fold_rollout_tail(tail.as_bytes(), Some(5)).status,
                Status::NeedsAnswer,
                "event kind {kind}"
            );
        }
    }

    #[test]
    fn fold_garbage_lines_are_skipped_not_fatal() {
        let tail = ["not json at all".to_string(), event_line("task_complete")].join("\n");
        assert_eq!(
            fold_rollout_tail(tail.as_bytes(), Some(5)).status,
            Status::Idle
        );
    }

    #[test]
    fn fold_token_count_yields_tokens_and_ctx_pct_from_model_context_window() {
        let tail = token_count_line(120_000, 200_000);
        let facts = fold_rollout_tail(tail.as_bytes(), Some(5));
        assert_eq!(facts.tokens, Some(120_000));
        assert_eq!(facts.ctx_pct, Some(60), "120k of a 200k codex window = 60%");
    }

    #[test]
    fn fold_last_user_message_becomes_the_name_truncated_to_60() {
        let long = "x".repeat(80);
        let tail = user_message_line(&long);
        let facts = fold_rollout_tail(tail.as_bytes(), Some(5));
        assert_eq!(
            facts.name.as_deref().map(str::len),
            Some(60),
            "truncated to 60 chars"
        );
    }

    #[test]
    fn fold_last_user_message_uses_only_the_first_line() {
        // A multi-line prompt (e.g. pasted code) must never leak its second+ lines into the
        // name (spec 008) — `\\n` here is the JSON-escaped newline, i.e. a real '\n' once parsed.
        let tail = user_message_line("first line\\nsecond line should never appear");
        let facts = fold_rollout_tail(tail.as_bytes(), Some(5));
        assert_eq!(facts.name.as_deref(), Some("first line"));
    }

    // --- join_rollouts ---

    const SLACK_SECS: u64 = 600; // spec 008: the join liveness guard window

    fn codex_proc(cwd: &str, start_wallclock_secs: u64) -> CodexProc {
        CodexProc {
            cwd: cwd.to_string(),
            start_wallclock_secs,
        }
    }

    fn candidate(session_id: &str, cwd: &str, mtime_secs: u64) -> RolloutCandidate {
        RolloutCandidate {
            session_id: session_id.to_string(),
            cwd: cwd.to_string(),
            mtime_secs,
        }
    }

    #[test]
    fn join_picks_the_newest_same_cwd_candidate() {
        let procs = [codex_proc("/a", 1_000)];
        let rollouts = [candidate("old", "/a", 500), candidate("new", "/a", 900)];
        assert_eq!(
            join_rollouts(&procs, &rollouts),
            vec![Some(&rollouts[1])],
            "newest same-cwd rollout wins"
        );
    }

    #[test]
    fn join_never_joins_processes_sharing_a_cwd() {
        let procs = [codex_proc("/b", 1_000), codex_proc("/b", 1_000)];
        let rollouts = [candidate("only", "/b", 900)];
        assert_eq!(
            join_rollouts(&procs, &rollouts),
            vec![None, None],
            "two live processes sharing a cwd never guess which rollout is whose"
        );
    }

    #[test]
    fn join_rejects_a_rollout_older_than_start_minus_slack() {
        let procs = [codex_proc("/c", 10_000)];
        let rollouts = [candidate("stale", "/c", 10_000 - SLACK_SECS - 1)];
        assert_eq!(join_rollouts(&procs, &rollouts), vec![None]);
    }

    #[test]
    fn join_accepts_a_rollout_exactly_at_the_slack_boundary() {
        let procs = [codex_proc("/d", 10_000)];
        let rollouts = [candidate("boundary", "/d", 10_000 - SLACK_SECS)];
        assert_eq!(join_rollouts(&procs, &rollouts), vec![Some(&rollouts[0])]);
    }

    #[test]
    fn join_with_no_matching_cwd_is_unjoined() {
        let procs = [codex_proc("/e", 1_000)];
        let rollouts = [candidate("elsewhere", "/z", 900)];
        assert_eq!(join_rollouts(&procs, &rollouts), vec![None]);
    }

    // --- codex::scan integration ---

    const NO_PROMPT_YET: &str = "codex — no prompt yet";

    /// Build a fake `/proc/<pid>` for a Codex TUI: comm/cmdline/environ/stat + fd/1 and cwd
    /// symlinks (mirrors `discovery::tests::fake_proc`, extended for codex's own shape).
    fn fake_codex_proc(root: &Path, pid: u32, cwd: &Path, pane: u64, pts_num: &str) {
        let dir = root.join(pid.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("comm"), "codex\n").unwrap();
        std::fs::write(dir.join("cmdline"), b"codex\0").unwrap();
        std::fs::write(dir.join("environ"), format!("WEZTERM_PANE={pane}\0")).unwrap();
        std::fs::write(
            dir.join("stat"),
            format!("{pid} (codex) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 100 23"),
        )
        .unwrap();
        let fd = dir.join("fd");
        std::fs::create_dir_all(&fd).unwrap();
        std::os::unix::fs::symlink(format!("/dev/pts/{pts_num}"), fd.join("1")).unwrap();
        std::os::unix::fs::symlink(cwd, dir.join("cwd")).unwrap();
    }

    /// Write one rollout file: `session_meta` line 0 + whatever tail lines are given.
    fn write_rollout(codex_root: &Path, uuid: &str, cwd: &str, tail_lines: &[String]) {
        let dir = codex_root.join("sessions/2026/07/10");
        std::fs::create_dir_all(&dir).unwrap();
        let meta = format!(
            r#"{{"timestamp":"2026-07-10T00:00:00Z","type":"session_meta","payload":{{"id":"{uuid}","cwd":"{cwd}","originator":"codex-tui","cli_version":"0.144.1","source":"cli"}}}}"#
        );
        let mut lines = vec![meta];
        lines.extend_from_slice(tail_lines);
        std::fs::write(
            dir.join(format!("rollout-2026-07-10T00-00-00-{uuid}.jsonl")),
            lines.join("\n"),
        )
        .unwrap();
    }

    #[test]
    fn scan_joins_one_process_to_its_rollout_and_matches_the_pane() {
        let tmp = std::env::temp_dir().join(format!("fleet-codex-scan-{}", std::process::id()));
        let codex_root = tmp.join("codex");
        let proc_root = tmp.join("proc");
        let real_cwd = tmp.join("workdir");
        std::fs::create_dir_all(&real_cwd).unwrap();
        fake_codex_proc(&proc_root, 500, &real_cwd, 27, "8");
        write_rollout(
            &codex_root,
            "11111111-1111-1111-1111-111111111111",
            real_cwd.to_str().unwrap(),
            &[event_line("task_complete")],
        );
        let rows = scan(&codex_root, &proc_root, &[]);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(rows.len(), 1, "one codex TUI process, one row");
        let row = &rows[0];
        assert_eq!(row.account.as_deref(), Some("codex"));
        assert_eq!(
            row.pane, None,
            "wave 12: the Codex lane carries no CMUX_SURFACE_ID yet, so it never matches a \
             surface. Wave 14 sources it from procsrc like the Claude lane."
        );
        assert_eq!(
            row.pts, None,
            "and with no surface the highlight write-target guard withholds the pts"
        );
        assert_eq!(row.status, Status::Idle, "task_complete tail");
        assert_eq!(
            row.name, "codex (untitled)",
            "joined rollout with no user_message yet must not read \"no prompt yet\" — the \
             session IS mid-conversation, that label is reserved for no-rollout-joined rows"
        );
    }

    #[test]
    fn scan_rollouts_orders_by_mtime_not_filename_so_a_long_running_session_survives_the_cap() {
        let tmp = std::env::temp_dir().join(format!(
            "fleet-codex-rollout-mtime-cap-{}",
            std::process::id()
        ));
        let codex_root = tmp.join("codex");

        // A long-running session: its filename encodes an OLD start (2020, sorts last
        // alphabetically among 2026-dated filler rollouts) but it's still being appended to, so
        // its mtime is the freshest of all. Filename-descending sort would truncate it out of
        // MAX_ROLLOUTS; mtime-descending must keep it.
        let old_dir = codex_root.join("sessions/2020/01/01");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(
            old_dir.join("rollout-2020-01-01T00-00-00-long-running.jsonl"),
            r#"{"timestamp":"2020-01-01T00:00:00Z","type":"session_meta","payload":{"id":"long-running","cwd":"/a","originator":"codex-tui","cli_version":"0.144.1","source":"cli"}}"#,
        )
        .unwrap();
        let old_path = old_dir.join("rollout-2020-01-01T00-00-00-long-running.jsonl");
        let bumped = std::fs::metadata(&old_path).unwrap().modified().unwrap()
            + std::time::Duration::from_hours(1);
        std::fs::File::open(&old_path)
            .unwrap()
            .set_modified(bumped)
            .unwrap();

        // MAX_ROLLOUTS filler rollouts, all dated 2026 (newer filename than the long-running
        // one), filling the cap.
        for i in 0..MAX_ROLLOUTS {
            write_rollout(&codex_root, &format!("filler-{i:04}"), "/a", &[]);
        }

        let (candidates, _) = scan_rollouts(&codex_root);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(candidates.len(), MAX_ROLLOUTS, "cap still enforced");
        assert!(
            candidates.iter().any(|c| c.session_id == "long-running"),
            "the freshest-mtime rollout must survive the cap despite its old filename"
        );
    }

    #[test]
    fn scan_placeholder_row_when_no_rollout_is_joined() {
        let tmp =
            std::env::temp_dir().join(format!("fleet-codex-scan-noprompt-{}", std::process::id()));
        let codex_root = tmp.join("codex"); // no sessions dir at all
        let proc_root = tmp.join("proc");
        let real_cwd = tmp.join("workdir");
        std::fs::create_dir_all(&real_cwd).unwrap();
        fake_codex_proc(&proc_root, 600, &real_cwd, 0, "9");

        let rows = scan(&codex_root, &proc_root, &[]);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(
            rows.len(),
            1,
            "a codex TUI with no rollout still gets a placeholder row"
        );
        assert_eq!(rows[0].name, NO_PROMPT_YET);
        assert_eq!(rows[0].session_id, "codex-pid-600");
        assert_eq!(rows[0].status, Status::Idle, "no rollout joined = Idle");
    }
}
