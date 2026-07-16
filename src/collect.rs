//! The one sensor pipeline: scan sessions + telemetry + panes + codex → sorted board rows.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/collect.rs
//! Deps:    discovery, telemetry, board, codex, panes, paths, procsrc (all fetched/pure seams)
//! Tested:  n/a directly — its steps are table-tested in their own modules; this is the shared
//!          orchestration so `tui::sweep` and `snapshot::run` can never diverge (spec 009).
//!
//! Key responsibilities:
//! - Run the identical row-assembly both the live board and `fleet snapshot` need, once: Claude
//!   rows (`board::assemble`) then Codex rows (`codex::scan`) appended, sorted once
//!   (`board::sort_rows`) — snapshot and board come from THIS code, so numbers never disagree.
//!
//! Design constraints:
//! - Blocking fs work (`discovery::scan`, tail reads, `codex::scan`) — call inside
//!   `spawn_blocking`, never on the UI task.
//! - The process table and the pane list are both fetched ASYNC by the caller and handed in as
//!   plain results, so this stays sync and spawn-free (spec 011).
//! - The caches are borrowed for the whole call; the TUI passes its persistent ones (held under
//!   the sweep mutex), the snapshot passes fresh ones. Read-only over the fleet.

use std::path::Path;

use crate::board::{self, SessionRow};
use crate::discovery::{self, ScanStats};
use crate::error::AppResult;
use crate::panes::{PaneCache, PaneRow};
use crate::procsrc::ProcTable;
use crate::telemetry::{TailCache, Telemetry};
use crate::{codex, paths};

/// One full sensor pass, ready for the board or the snapshot.
#[derive(Debug)]
pub struct Collected {
    /// Assembled, sorted session rows (Claude + Codex, one sort).
    pub rows: Vec<SessionRow>,
    /// Discovery tallies (footer + doctor + snapshot exit code).
    pub stats: ScanStats,
    /// A degraded lane (e.g. wezterm unreachable) — rows are still valid.
    pub lane_error: Option<String>,
    /// Live Codex rows folded into `rows` this pass (footer `· N codex`).
    pub codex_count: usize,
}

/// Scan + fold the whole fleet into sorted rows, reusing the given caches. `panes_result` and
/// `procs_result` are already fetched by the caller (both off the blocking task).
pub fn collect(
    tails: &mut TailCache,
    pane_cache: &mut PaneCache,
    panes_result: AppResult<(Vec<PaneRow>, Option<String>)>,
    procs_result: AppResult<ProcTable>,
) -> Collected {
    let claude_dir = paths::claude_dir();
    // A failed `ps` is NOT an empty fleet: liveness is unknowable, so every session would read
    // as dead. Flag it loudly rather than render a clean, empty, wrong board.
    let (procs, proc_error) = match procs_result {
        Ok(table) => (table, None),
        Err(e) => (ProcTable::new(), Some(format!("process table: {e}"))),
    };
    let (sessions, mut stats) = discovery::scan(&claude_dir.join("sessions"), &procs);
    stats.procs_unavailable = proc_error.is_some();
    let projects = claude_dir.join("projects");
    let telemetry: Vec<Telemetry> = sessions
        .iter()
        .map(|s| tails.read(&projects, &s.file.cwd, &s.file.session_id))
        .collect();
    let live_ids: Vec<&str> = sessions
        .iter()
        .map(|s| s.file.session_id.as_str())
        .collect();
    tails.retain(&live_ids);
    let (pane_rows, pane_error) = pane_cache.fold(panes_result);
    // A dead process table empties the board; a degraded pane lane only costs the PANE column.
    // Surface the more severe one.
    let lane_error = proc_error.or(pane_error);
    let mut rows = board::assemble(&sessions, &telemetry, &pane_rows);
    let codex_rows = codex::scan(&paths::codex_dir(), Path::new("/proc"), &pane_rows);
    let codex_count = codex_rows.len();
    rows.extend(codex_rows);
    board::sort_rows(&mut rows);
    Collected {
        rows,
        stats,
        lane_error,
        codex_count,
    }
}
