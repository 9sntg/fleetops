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
use crate::cmux::{Surface, SurfaceCache};
use crate::discovery::{self, ScanStats};
use crate::error::AppResult;
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
    /// A degraded lane (e.g. cmux unreachable) — rows are still valid.
    pub lane_error: Option<String>,
    /// Live Codex rows folded into `rows` this pass (footer `· N codex`).
    pub codex_count: usize,
}

/// Scan + fold the whole fleet into sorted rows, reusing the given caches. `panes_result` and
/// `procs_result` are already fetched by the caller (both off the blocking task).
pub fn collect(
    tails: &mut TailCache,
    surface_cache: &mut SurfaceCache,
    surfaces_result: AppResult<Vec<Surface>>,
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
    let (surfaces, surface_error) = surface_cache.fold(surfaces_result);
    // A dead process table empties the board; a degraded cmux lane only costs the PANE column.
    // Surface the more severe one.
    let lane_error = proc_error.or(surface_error);
    let mut rows = board::assemble(&sessions, &telemetry, &surfaces);
    let codex_rows = codex::scan(&paths::codex_dir(), Path::new("/proc"), &surfaces);
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
