//! The TUI event loop + the sensor sweep. Only I/O site of the board.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/tui/mod.rs
//! Deps:    ratatui (init/restore + panic hook), crossterm (EventStream), tokio;
//!          collect, board, panes, highlight, runner
//! Tested:  the seams are tested in keys/model/view/board/codex/collect/…; this loop is the thin I/O shell.
//!
//! Key responsibilities:
//! - Own the terminal (via `ratatui::try_init`/`try_restore` — installs the panic hook) and the loop.
//! - `sweep`: one sensor pass — wezterm list (async, bounded) handed to `collect::collect`
//!   (blocking fs via `spawn_blocking`: sessions scan + transcript tails + Codex rows, sorted
//!   once) → `Snapshot` over the mpsc channel. `collect` is the SAME code `fleet snapshot` runs
//!   (spec 009), so the board and the snapshot never disagree.
//!
//! Design constraints:
//! - Async work never runs on the UI task — sweeps and jumps are spawned; the loop only `select!`s.
//! - Read-only over the fleet; the only mutating verbs are `activate-tab`/`-pane` (focus) and
//!   the OSC 11/111 pane-tint writes (spec 006).

pub mod keys;
pub mod model;
pub mod view;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::cmux::SurfaceCache;
use crate::error::AppResult;
use crate::runner::{Exec, Runner};
use crate::telemetry::TailCache;
use crate::{board, cmux, collect, highlight, procsrc};

use model::{App, Msg, Snapshot};

/// Per-loop sensor caches shared across sweeps (behind one mutex; sweeps are short).
#[derive(Debug, Default)]
struct SweepCaches {
    tails: TailCache,
    surfaces: SurfaceCache,
}

/// Sensor sweep cadence (spec 001: ~2 s).
const POLL: Duration = Duration::from_secs(2);
/// Footer age / redraw tick.
const TICK: Duration = Duration::from_secs(1);

/// Run the board until the user quits. `highlight_enabled` gates the OSC pane-tint writes
/// (`fleet --no-highlight`) — the model still computes them, the loop just drops them.
pub async fn run(highlight_enabled: bool) -> AppResult<()> {
    let mut terminal = ratatui::try_init()?;
    let result = event_loop(&mut terminal, highlight_enabled).await;
    if let Err(e) = ratatui::try_restore() {
        // A swallowed restore failure leaves a garbled raw-mode terminal with zero explanation.
        eprintln!("fleet: terminal restore failed: {e} — run `reset`");
    }
    result
}

async fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    highlight_enabled: bool,
) -> AppResult<()> {
    let runner: Arc<dyn Runner> = Arc::new(Exec);
    let cache = Arc::new(Mutex::new(SweepCaches::default()));
    let (tx, mut rx) = mpsc::channel::<Msg>(16);
    // Sweeps overlap (5 s wezterm timeout vs 2 s cadence); each carries a monotone seq so the
    // model can drop a late-finishing older sweep instead of stomping fresh data.
    let mut sweep_seq: u64 = 0;

    let mut app = App::default();
    let mut events = EventStream::new();
    // The interval's built-in immediate first tick IS the t=0 sweep.
    let mut poll = tokio::time::interval(POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Every loop exit — clean quit or a dying event stream — funnels through this instead of an
    // early `return`, so the post-loop highlight cleanup below always runs first (spec 006: quit
    // always resets).
    let mut outcome: AppResult<()> = Ok(());

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
                // A dying event stream (tty EIO) must not look like a clean `q` — exit nonzero,
                // but only after the cleanup below runs like every other quit path.
                Some(Err(e)) => {
                    outcome = Err(e.into());
                    break;
                }
                None => break,
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
            // Coalesce: held-down 'r' (autorepeat ~30 Hz) must not stack a subprocess fan-out
            // per keypress — one manual sweep in flight at a time.
            if app.sweeps_settled(sweep_seq) {
                sweep_seq += 1;
                spawn_sweep(&runner, &cache, &tx, sweep_seq);
            }
        }
        if let Some(target) = app.pending_jump.take() {
            spawn_jump(&runner, &tx, target);
        }
        if !app.pending_highlights.is_empty() {
            let cmds = std::mem::take(&mut app.pending_highlights);
            if highlight_enabled {
                highlight::spawn_apply(cmds);
            }
        }
        if app.should_quit {
            break;
        }
    }
    if highlight_enabled {
        highlight::reset_all(app.tinted_pts()).await;
    }
    outcome
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
/// The row assembly itself is `collect::collect` — the SAME code `fleet snapshot` runs, so the
/// board and the snapshot can never disagree (spec 009).
async fn sweep(runner: &dyn Runner, cache: &Arc<Mutex<SweepCaches>>) -> Result<Snapshot, String> {
    // Independent lanes — fetch concurrently.
    let (surfaces_result, procs_result) = tokio::join!(cmux::list(runner), procsrc::fetch(runner));
    let cache = Arc::clone(cache);
    tokio::task::spawn_blocking(move || {
        let mut guard = match cache.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(), // caches are best-effort; still usable
        };
        // `collect` is the SAME pipeline `snapshot::run` uses, so the board and the snapshot
        // can never disagree (spec 009). The caches are held only for its duration, then dropped.
        let SweepCaches { tails, surfaces } = &mut *guard;
        let collected = collect::collect(tails, surfaces, surfaces_result, procs_result);
        drop(guard); // release the caches before returning (lock-contention hygiene)
        Snapshot {
            seq: 0, // stamped by spawn_sweep
            rows: collected.rows,
            stats: collected.stats,
            lane_error: collected.lane_error,
            codex_count: collected.codex_count,
        }
    })
    .await
    .map_err(|e| format!("sweep task: {e}"))
}

/// Fire the jump off the UI task; only failures surface (as a footer `Msg::Error`).
/// One call: cmux's `focus` brings the surface's window to the front AND focuses it, so the
/// wezterm-era activate-tab→activate-pane ordering hazard is gone (spec 012).
fn spawn_jump(runner: &Arc<dyn Runner>, tx: &mpsc::Sender<Msg>, target: board::MatchedPane) {
    let runner = Arc::clone(runner);
    let tx = tx.clone();
    tokio::spawn(async move {
        if let Err(e) = cmux::focus(runner.as_ref(), &target.surface_id).await {
            let _ = tx.send(Msg::Error(format!("jump: {e}"))).await;
        }
    });
}
