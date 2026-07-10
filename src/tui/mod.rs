//! The TUI event loop + the sensor sweep. Only I/O site of the board.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/tui/mod.rs
//! Deps:    ratatui (init/restore + panic hook), crossterm (EventStream), tokio;
//!          discovery, telemetry, board, panes, runner, paths
//! Tested:  the seams are tested in keys/model/view/board/…; this loop is the thin I/O shell.
//!
//! Key responsibilities:
//! - Own the terminal (via `ratatui::try_init`/`try_restore` — installs the panic hook) and the loop.
//! - `sweep`: one sensor pass — wezterm list (async, bounded) + sessions scan + transcript tails
//!   (blocking fs via `spawn_blocking`) → assembled `Snapshot` over the mpsc channel.
//!
//! Design constraints:
//! - Async work never runs on the UI task — sweeps and jumps are spawned; the loop only `select!`s.
//! - Read-only over the fleet; the only mutating verb is `activate-pane` (focus).

pub mod keys;
pub mod model;
pub mod view;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::error::AppResult;
use crate::panes::PaneCache;
use crate::runner::{Exec, Runner};
use crate::telemetry::TailCache;
use crate::{board, discovery, panes, paths};

use model::{App, Msg, Snapshot};

/// Per-loop sensor caches shared across sweeps (behind one mutex; sweeps are short).
#[derive(Debug, Default)]
struct SweepCaches {
    tails: TailCache,
    panes: PaneCache,
}

/// Sensor sweep cadence (spec 001: ~2 s).
const POLL: Duration = Duration::from_secs(2);
/// Footer age / redraw tick.
const TICK: Duration = Duration::from_secs(1);

/// Run the board until the user quits.
pub async fn run() -> AppResult<()> {
    let mut terminal = ratatui::try_init()?;
    let result = event_loop(&mut terminal).await;
    let _ = ratatui::try_restore();
    result
}

async fn event_loop(terminal: &mut ratatui::DefaultTerminal) -> AppResult<()> {
    let runner: Arc<dyn Runner> = Arc::new(Exec);
    let cache = Arc::new(Mutex::new(SweepCaches::default()));
    let (tx, mut rx) = mpsc::channel::<Msg>(16);
    // Sweeps overlap (5 s wezterm timeout vs 2 s cadence); each carries a monotone seq so the
    // model can drop a late-finishing older sweep instead of stomping fresh data.
    let mut sweep_seq: u64 = 0;
    sweep_seq += 1;
    spawn_sweep(&runner, &cache, &tx, sweep_seq);

    let mut app = App::default();
    let mut events = EventStream::new();
    let mut poll = tokio::time::interval(POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    poll.tick().await; // consume the immediate first tick; spawn_sweep above covers t=0
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        terminal.draw(|f| view::render(f, &app))?;

        tokio::select! {
            maybe = events.next() => match maybe {
                Some(Ok(Event::Key(key))) => {
                    if let Some(action) = keys::map(key) {
                        app.update(Msg::Key(action));
                    }
                }
                Some(Ok(_)) => {} // resize etc. — next draw picks it up
                Some(Err(_)) | None => break,
            },
            Some(msg) = rx.recv() => app.update(msg),
            _ = poll.tick() => {
                sweep_seq += 1;
                spawn_sweep(&runner, &cache, &tx, sweep_seq);
            }
            _ = tick.tick() => app.update(Msg::Tick),
        }

        if app.refresh_requested {
            app.refresh_requested = false;
            sweep_seq += 1;
            spawn_sweep(&runner, &cache, &tx, sweep_seq);
        }
        if let Some((tab_id, pane_id)) = app.pending_jump.take() {
            spawn_jump(&runner, &tx, tab_id, pane_id);
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}

/// Run one full sensor sweep off the UI task; the result arrives as a `Msg` carrying `seq`.
fn spawn_sweep(
    runner: &Arc<dyn Runner>,
    cache: &Arc<Mutex<SweepCaches>>,
    tx: &mpsc::Sender<Msg>,
    seq: u64,
) {
    let runner = Arc::clone(runner);
    let cache = Arc::clone(cache);
    let tx = tx.clone();
    tokio::spawn(async move {
        let msg = match sweep(runner.as_ref(), &cache).await {
            Ok(mut snapshot) => {
                snapshot.seq = seq;
                Msg::Snapshot(Box::new(snapshot))
            }
            Err(e) => Msg::Error(e),
        };
        let _ = tx.send(msg).await; // loop gone = shutting down
    });
}

/// One sensor pass: panes (async) + sessions/transcripts (blocking) → assembled snapshot.
/// A degraded wezterm lane is a `lane_error`, not a failure — session rows still ship, and the
/// LAST GOOD pane list keeps TAB/PANE matches alive (stale beats blank; the footer says so).
async fn sweep(runner: &dyn Runner, cache: &Arc<Mutex<SweepCaches>>) -> Result<Snapshot, String> {
    let panes_result = panes::list_panes(runner).await;
    let cache = Arc::clone(cache);
    tokio::task::spawn_blocking(move || {
        let claude_dir = paths::claude_dir();
        let (sessions, stats) =
            discovery::scan(&claude_dir.join("sessions"), std::path::Path::new("/proc"));
        let projects = claude_dir.join("projects");
        let mut guard = match cache.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(), // caches are best-effort; still usable
        };
        let telemetry: Vec<crate::telemetry::Telemetry> = sessions
            .iter()
            .map(|s| guard.tails.read(&projects, &s.file.cwd, &s.file.session_id))
            .collect();
        let live_ids: Vec<&str> = sessions
            .iter()
            .map(|s| s.file.session_id.as_str())
            .collect();
        guard.tails.retain(&live_ids);
        let (pane_rows, lane_error) = guard.panes.fold(panes_result);
        Snapshot {
            seq: 0, // stamped by spawn_sweep
            rows: board::assemble(&sessions, &telemetry, &pane_rows),
            stats,
            lane_error,
        }
    })
    .await
    .map_err(|e| format!("sweep task: {e}"))
}

/// Fire the jump off the UI task; only failures surface (as a footer `Msg::Error`).
/// Tab first, then pane: activate-pane alone focuses within a tab but doesn't switch tabs.
fn spawn_jump(runner: &Arc<dyn Runner>, tx: &mpsc::Sender<Msg>, tab_id: u64, pane_id: u64) {
    let runner = Arc::clone(runner);
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = async {
            runner.run(&panes::activate_tab_spec(tab_id)).await?;
            runner.run(&panes::activate_pane_spec(pane_id)).await
        }
        .await;
        if let Err(e) = result {
            let _ = tx.send(Msg::Error(format!("jump: {e}"))).await;
        }
    });
}
