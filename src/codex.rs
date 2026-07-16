//! codex ctx: Codex CLI TUI sessions on the board — recognize the process, join its rollout,
//! fold status/tokens/name from the tail. All pure except `scan` (spec 008).
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/codex.rs
//! Deps:    serde/serde_json (rollout JSON); std::fs (via `scan`, called by the sensor's
//!          spawn_blocking); board (SessionRow, match_surface); procsrc (ProcTable); runner
//!          (the ps/lsof seam); fold (Status, STALL_AFTER_SECS); cmux (Surface)
//! Tested:  inline `#[cfg(test)]` — synthetic rollout JSONL lines + tempdir fake `/proc` +
//!          `~/.codex/sessions` tree (house pattern, see discovery.rs/telemetry.rs)
//!
//! Key responsibilities:
//! - Recognize a Codex TUI process: argv0-only whose BASENAME is `codex`, owning a tty
//!   (`is_codex_tui`) — the node shim (argv0 `node`) and `codex exec`/`--version` are skipped
//!   (basename mismatch / extra argv). macOS `ps` reports comm as a FULL PATH, so Linux's
//!   `comm == "codex"` test would be false for every process — see `is_codex_tui` (spec 014).
//! - Parse a rollout's `session_meta` line 0 (`parse_session_meta`) and fold its tail
//!   (`fold_rollout_tail`) into status/tokens/ctx%/name per the spec 008 status table.
//! - Join each live process to its newest same-cwd rollout, without sqlite (v1): a liveness
//!   guard rejects a rollout mtime older than the process's own start minus a slack window; two
//!   processes sharing a cwd never join (`join_rollouts` — never guess, house rule).
//! - `fetch`: the async process lane — `ps -Awwo pid=,args=` for the gate, then ONE batched
//!   `lsof -d cwd` for the join key (macOS ps has no cwd field).
//! - `scan`: the one fs-touching entry point, mirroring `discovery::scan`'s shape — walks
//!   `codex_root/sessions/**/rollout-*.jsonl` for candidates, joins them to the fetched
//!   processes, and assembles `SessionRow`s (matched via `board::match_surface`).
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
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use crate::board::{self, SessionRow};
use crate::cmux::Surface;
use crate::error::AppResult;
use crate::fold::{self, Status};
use crate::git;
use crate::procsrc::ProcTable;
use crate::runner::{CommandSpec, Runner};

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

/// A Codex TUI process: argv is exactly one token whose basename is `codex`, and it owns a tty.
///
/// macOS shape, verified live 2026-07-16 — this is NOT Linux's rule and the difference is the
/// whole reason wave 14 could not be written blind:
/// - Linux read `/proc/<pid>/comm`, the bare basename `"codex"`. macOS `ps` reports the FULL
///   PATH (`/Users/…/vendor/aarch64-apple-darwin/bin/codex`), so a `== "codex"` test is false
///   for every process, forever — the lane would find nothing and look exactly like "no Codex
///   running". Hence `basename`.
/// - The node shim (`node /…/bin/codex`) shares the real binary's tty AND cwd, so it must be
///   excluded or the cwd join goes ambiguous and drops both: its argv0 basename is `node`.
/// - `codex exec …` / `codex --version` carry extra argv tokens.
pub fn is_codex_tui(argv: &str, tty: Option<&str>) -> bool {
    let mut tokens = argv.split_ascii_whitespace();
    let Some(argv0) = tokens.next() else {
        return false;
    };
    let argv0_only = tokens.next().is_none();
    let basename = argv0.rsplit('/').next().unwrap_or(argv0);
    basename == "codex" && argv0_only && tty.is_some()
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
/// One live Codex TUI process's already-fetched facts (scan-internal; `CodexProc` is the pure
/// join input derived from this). Fetched by `fetch`, off the blocking task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pid: u32,
    cwd: String,
    pts: Option<String>,
    surface_id: Option<String>,
    /// Seconds since this process started (`ps etime=`); `scan` turns it into an epoch.
    elapsed_secs: u64,
}

/// Scan `codex_root` for rollouts, join them to the already-fetched live Codex TUI processes,
/// and return assembled `SessionRow`s — matched against `surfaces` via `board::match_surface`.
/// Blocking fs work — the sensor calls this inside `spawn_blocking`, same pattern as
/// `discovery::scan`; `proc_infos` comes from `fetch`, off the blocking task.
pub fn scan(codex_root: &Path, proc_infos: &[ProcInfo], surfaces: &[Surface]) -> Vec<SessionRow> {
    if proc_infos.is_empty() {
        return Vec::new(); // no Codex running — skip the rollout walk entirely
    }
    let (candidates, paths_by_id) = scan_rollouts(codex_root);
    // The one impure edge (spec 008): rollout age is computed against wallclock now, here only.
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let join_procs: Vec<CodexProc> = proc_infos
        .iter()
        .map(|p| CodexProc {
            cwd: p.cwd.clone(),
            // Linux derived this from btime + starttime/HZ with a hardcoded HZ guess; macOS just
            // subtracts the elapsed time ps already reports (spec 014).
            start_wallclock_secs: now_secs.saturating_sub(p.elapsed_secs),
        })
        .collect();
    let joined = join_rollouts(&join_procs, &candidates);

    proc_infos
        .iter()
        .zip(joined)
        .map(|(proc, matched)| {
            let shares_cwd = proc_infos.iter().filter(|p| p.cwd == proc.cwd).count() > 1;
            build_row(now_secs, proc, matched, &paths_by_id, shares_cwd, surfaces)
        })
        .collect()
}

/// The Codex process-discovery call: pid + full argv for every process.
///
/// Deliberately NOT `-E`: the Claude lane's table needs environments (for `CMUX_SURFACE_ID`), but
/// this lane only needs argv, and `ps -E` appends the environment to the command with no
/// delimiter — argv and environ would be indistinguishable, and the gate counts argv tokens.
/// Keeping `-E` off makes the token count exact AND keeps every process's secrets out of reach.
pub fn procs_spec() -> CommandSpec {
    CommandSpec {
        program: "ps".to_string(),
        args: vec!["-Awwo".to_string(), "pid=,args=".to_string()],
        env: Vec::new(),
        timeout: Duration::from_secs(5),
    }
}

/// The cwd call — the rollout join key. macOS `ps` has no cwd field, so `lsof` supplies it; one
/// batched call for all candidate pids, never one per pid.
pub fn cwds_spec(pids: &[u32]) -> CommandSpec {
    let csv = pids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    CommandSpec {
        program: "lsof".to_string(),
        args: vec![
            "-a".to_string(),
            "-d".to_string(),
            "cwd".to_string(),
            "-p".to_string(),
            csv,
            "-Fpn".to_string(),
        ],
        env: Vec::new(),
        timeout: Duration::from_secs(5),
    }
}

/// Parse `ps -Awwo pid=,args=` → the pids that pass the Codex TUI argv gate, with their argv.
/// `tty_of` supplies each pid's tty from the shared process table.
pub fn parse_procs(bytes: &[u8], tty_of: &dyn Fn(u32) -> Option<String>) -> Vec<u32> {
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let (pid, argv) = line.split_once(char::is_whitespace)?;
            let pid: u32 = pid.parse().ok()?;
            is_codex_tui(argv.trim(), tty_of(pid).as_deref()).then_some(pid)
        })
        .collect()
}

/// Parse `lsof -Fpn` field output: `p<pid>` lines followed by `n<path>` lines.
pub fn parse_cwds(bytes: &[u8]) -> HashMap<u32, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = HashMap::new();
    let mut current: Option<u32> = None;
    for line in text.lines() {
        if let Some(pid) = line.strip_prefix('p') {
            current = pid.trim().parse().ok();
        } else if let Some(path) = line.strip_prefix('n') {
            if let Some(pid) = current {
                // First n-line per pid wins; `-d cwd` yields exactly one.
                out.entry(pid).or_insert_with(|| path.trim().to_string());
            }
        }
    }
    out
}

/// Fetch live Codex TUI processes: the argv gate, then one batched `lsof` for their cwds.
/// Async — the caller runs this off the blocking task and hands the result to `scan`, exactly
/// like the Claude lane's process table and the cmux topology.
pub async fn fetch(runner: &dyn Runner, table: &ProcTable) -> AppResult<Vec<ProcInfo>> {
    let bytes = runner.run(&procs_spec()).await?;
    let pids = parse_procs(&bytes, &|pid| table.get(&pid).and_then(|f| f.tty.clone()));
    if pids.is_empty() {
        return Ok(Vec::new()); // no Codex running is the common case, and costs no lsof
    }
    let cwd_bytes = runner.run(&cwds_spec(&pids)).await?;
    let cwds = parse_cwds(&cwd_bytes);
    Ok(pids
        .into_iter()
        .filter_map(|pid| {
            let facts = table.get(&pid)?;
            Some(ProcInfo {
                pid,
                // No cwd -> no join key -> the row would be unjoinable anyway.
                cwd: cwds.get(&pid)?.clone(),
                pts: facts.tty.clone(),
                surface_id: facts.surface_id.clone(),
                elapsed_secs: facts.elapsed_secs,
            })
        })
        .collect())
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
    // This lane already touches the fs (rollout walk), so it resolves its own branch.
    let branch = git::branch_of(Path::new(&proc.cwd));
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
            stream: pane.as_ref().map(|p| p.stream.clone()),
            branch,
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
        stream: pane.as_ref().map(|p| p.stream.clone()),
        branch,
        pane,
        pts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fold::STALL_AFTER_SECS;

    // --- is_codex_tui ---

    /// The real macOS argv0, captured live 2026-07-16 from a running Codex TUI.
    const REAL_ARGV0: &str = "/Users/user/.nvm/versions/node/v24.18.0/lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex";

    #[test]
    fn is_codex_tui_table() {
        let node_shim = "node /Users/user/.nvm/versions/node/v24.18.0/bin/codex";
        let exec_form = format!("{REAL_ARGV0} exec --json 'do a thing'");
        let cases: &[(&str, Option<&str>, bool)] = &[
            // The interactive TUI as macOS actually reports it: argv0 is the FULL PATH to the
            // vendored binary, alone, on a tty. Linux's `comm == "codex"` test would reject this.
            (REAL_ARGV0, Some("/dev/ttys011"), true),
            // A short path still works — the gate is on the basename, not the whole string.
            ("/opt/homebrew/bin/codex", Some("/dev/ttys004"), true),
            // The node shim: shares the real binary's tty AND cwd, so it MUST be excluded or the
            // cwd join sees two processes and drops both. argv0's basename is `node`.
            (node_shim, Some("/dev/ttys011"), false),
            // `codex exec …` — extra argv, transient, must be skipped.
            (&exec_form, Some("/dev/ttys011"), false),
            (
                "/opt/homebrew/bin/codex --version",
                Some("/dev/ttys004"),
                false,
            ),
            // No tty (piped/daemonized) — never a TUI target.
            (REAL_ARGV0, None, false),
            // Not codex at all.
            ("/bin/zsh -l", Some("/dev/ttys004"), false),
            ("", Some("/dev/ttys004"), false),
        ];
        for (argv, tty, want) in cases {
            assert_eq!(is_codex_tui(argv, *tty), *want, "argv={argv:?} tty={tty:?}");
        }
    }

    #[test]
    fn the_full_path_comm_is_why_linuxs_rule_could_not_be_reused() {
        // Regression pin for spec 014's central finding. On Linux `/proc/<pid>/comm` is the bare
        // basename, so the gate was `comm == "codex"`. macOS reports the full path, so that test
        // is false for EVERY process — the lane would find nothing and be indistinguishable from
        // "no Codex running". This is the assertion that would have caught a blind port.
        assert_ne!(
            REAL_ARGV0, "codex",
            "macOS never hands back a bare basename"
        );
        assert!(is_codex_tui(REAL_ARGV0, Some("/dev/ttys011")));
    }

    // --- the macOS process lane (ps + lsof) ---

    const PS_ARGS_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/ps-codex-args.txt");
    const LSOF_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/lsof-cwd.txt");

    #[test]
    fn ps_fixture_finds_the_real_binary_and_rejects_the_shim() {
        // Captured live 2026-07-16 from a running `codex` TUI: the real vendored binary (51450)
        // and the node shim that launched it (51448). Both are on the SAME tty and the SAME cwd,
        // so the shim must be rejected by argv alone or the cwd join sees two and drops both.
        let tty = |_pid: u32| Some("/dev/ttys011".to_string());
        let pids = parse_procs(PS_ARGS_FIXTURE, &tty);
        assert_eq!(pids, vec![51450], "only the real codex binary");
    }

    #[test]
    fn ps_gate_requires_a_tty_so_a_piped_codex_is_not_a_tui() {
        let no_tty = |_pid: u32| None;
        assert!(parse_procs(PS_ARGS_FIXTURE, &no_tty).is_empty());
    }

    #[test]
    fn ps_rows_that_are_garbage_are_skipped_not_fatal() {
        let tty = |_pid: u32| Some("/dev/ttys001".to_string());
        let input = b"not-a-pid /opt/homebrew/bin/codex\n\n99\n  7 /opt/homebrew/bin/codex\n";
        assert_eq!(parse_procs(input, &tty), vec![7]);
    }

    #[test]
    fn lsof_fixture_yields_the_cwd_join_key() {
        // Captured live 2026-07-16. `-Fpn` emits p<pid> then n<path> per process.
        let cwds = parse_cwds(LSOF_FIXTURE);
        assert_eq!(cwds.get(&51450).map(String::as_str), Some("/private/tmp"));
        assert_eq!(cwds.get(&51448).map(String::as_str), Some("/private/tmp"));
        assert_eq!(cwds.len(), 2);
    }

    #[test]
    fn lsof_garbage_is_empty_not_a_panic() {
        assert!(parse_cwds(b"").is_empty());
        assert!(parse_cwds(b"total nonsense").is_empty());
        assert!(
            parse_cwds(b"n/orphan/path\n").is_empty(),
            "an n-line with no preceding p-line names no process"
        );
    }

    #[test]
    fn specs_are_explicit_argv_and_the_ps_call_never_asks_for_environments() {
        let spec = procs_spec();
        assert_eq!(spec.program, "ps");
        assert_eq!(spec.args, vec!["-Awwo", "pid=,args="]);
        assert!(
            !spec.args[0].contains('E'),
            "-E would append the environment to the command with no delimiter, making the argv \
             token count (the whole gate) meaningless — and would pull every process's secrets in"
        );
        let cwds = cwds_spec(&[7, 9]);
        assert_eq!(cwds.program, "lsof");
        assert!(cwds.args.contains(&"7,9".to_string()), "one batched call");
    }

    #[tokio::test]
    async fn fetch_skips_lsof_entirely_when_no_codex_is_running() {
        use crate::runner::CannedRunner;
        let runner = CannedRunner::new(b"  888 /bin/zsh -l\n".to_vec());
        let table = ProcTable::new();
        let procs = fetch(&runner, &table).await.expect("ok");
        assert!(procs.is_empty());
        assert_eq!(
            runner.all_specs().len(),
            1,
            "no Codex is the common case; it must not cost an lsof"
        );
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

    /// A live Codex TUI process, as `codex::fetch` would have returned it.
    fn live_proc(pid: u32, cwd: &Path, pts: &str, elapsed_secs: u64) -> ProcInfo {
        ProcInfo {
            pid,
            cwd: cwd.to_str().unwrap().to_string(),
            pts: Some(pts.to_string()),
            surface_id: None,
            elapsed_secs,
        }
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
    fn scan_joins_one_process_to_its_rollout_and_matches_the_surface() {
        let tmp = std::env::temp_dir().join(format!("fleet-codex-scan-{}", std::process::id()));
        let codex_root = tmp.join("codex");
        let real_cwd = tmp.join("workdir");
        std::fs::create_dir_all(&real_cwd).unwrap();
        let mut proc = live_proc(500, &real_cwd, "/dev/ttys008", 30);
        proc.surface_id = Some("uuid-codex".to_string());
        write_rollout(
            &codex_root,
            "11111111-1111-1111-1111-111111111111",
            real_cwd.to_str().unwrap(),
            &[event_line("task_complete")],
        );
        let surfaces = [Surface {
            id: "uuid-codex".to_string(),
            window_index: 1,
            tab_index: 3,
            name: "codex-stream".to_string(),
            cwd: String::new(),
        }];
        let rows = scan(&codex_root, &[proc], &surfaces);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(rows.len(), 1, "one codex TUI process, one row");
        let row = &rows[0];
        assert_eq!(row.account.as_deref(), Some("codex"));
        assert_eq!(
            row.pane.as_ref().map(|p| p.tab_index),
            Some(3),
            "a Codex session under cmux matches its surface by exact id, same as Claude"
        );
        assert_eq!(
            row.stream.as_deref(),
            Some("codex-stream"),
            "spec 015: STREAM applies to Codex rows too"
        );
        assert_eq!(
            row.pts.as_deref(),
            Some("/dev/ttys008"),
            "with a surface, the highlight write-target guard lets the tty through"
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
        let real_cwd = tmp.join("workdir");
        std::fs::create_dir_all(&real_cwd).unwrap();
        let proc = live_proc(600, &real_cwd, "/dev/ttys009", 12);

        let rows = scan(&codex_root, &[proc], &[]);
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
