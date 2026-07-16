//! highlight ctx: pure tint math + exact OSC escape bytes, plus the thin OSC writer, for
//! wave-6 pane highlighting.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/highlight.rs
//! Deps:    fold (Status); tokio (spawn/spawn_blocking/time) for the writer
//! Tested:  inline `#[cfg(test)]` — desired_tint/tint_color tables, osc_set/osc_reset exact
//!          bytes; one `#[ignore]` live-verify harness gated on `FLEET_PROBE_PTS`. The writer
//!          itself is I/O and covered by that live-verify harness plus the commands `model`
//!          table-tests it into producing.
//!
//! Key responsibilities:
//! - `desired_tint`: Status -> Tint (spec 006 table). `Idle` maps to `None` here — green
//!   stickiness/the finish-pulse transition is the model's job, it owns prev-status.
//! - Exact OSC 11 (set) / OSC 111 (reset) escape bytes — verified live through ConPTY on the
//!   original WSL2 box (probe 2026-07-10, `plans/002-pane-highlight/`), and re-verified against
//!   a real Darwin pty in wave 13.
//! - `HighlightCmd`: what the loop must write, and to which pts.
//! - `spawn_apply`/`reset_all`: the detached writer — best-effort pts writes, never surfaced
//!   as an error (a dead pane is normal).
//!
//! Design constraints:
//! - Pure core (`Tint`/`HighlightCmd`/`desired_tint`/`tint_color`/`osc_set`/`osc_reset`) — no
//!   I/O, no tokio.
//! - The writer never blocks the UI task: opens/writes run inside `spawn_blocking`; `Pulse`
//!   frame delays use `tokio::time::sleep`, never `std::thread::sleep`.
//! - `reset_all` is timeout-bounded — quit must never hang on a wedged/closed pts.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;

use crate::fold::Status;

/// OSC 11 hex payload — amber (`NeedsAnswer` / `Waiting`).
pub const AMBER: &str = "453000";
/// OSC 11 hex payload — dark red (`Stalled`).
pub const RED: &str = "3a0d0d";
/// OSC 11 hex payload — steady dark green (sticky "just finished, not yet noticed").
pub const GREEN: &str = "0a3512";
/// OSC 11 hex payload — bright green, the pulse's bright frame.
pub const PULSE_BRIGHT: &str = "1a7a30";

/// Pane background tint driven by board status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    /// No tint — write OSC 111 to restore the terminal's configured default.
    None,
    /// `NeedsAnswer` / `Waiting`.
    Amber,
    /// `Stalled`.
    Red,
    /// Steady state after the finish pulse settles; sticky until "noticed" (spec 006).
    Green,
}

/// A highlight write the loop must apply to one session's pts (spec 006 §Seams).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HighlightCmd {
    /// Hold a steady tint. `tint: Tint::None` writes an OSC 111 reset instead of a color.
    Steady {
        /// Target pts path (e.g. `/dev/ttys007`).
        pts: String,
        /// Desired steady tint.
        tint: Tint,
    },
    /// The "just finished" pulse: bright/dim green alternation (~1 s) settling into steady
    /// dark green. The frame timing lives in the writer, not here.
    Pulse {
        /// Target pts path.
        pts: String,
    },
}

/// Status -> desired tint (spec 006 table). Pure lookup; no transition/stickiness memory —
/// that belongs to the model, which diffs against the previous sweep's status per session.
pub const fn desired_tint(status: Status) -> Tint {
    match status {
        Status::NeedsAnswer | Status::Waiting => Tint::Amber,
        Status::Stalled => Tint::Red,
        Status::Working | Status::Idle | Status::Shell | Status::Unknown => Tint::None,
    }
}

/// The hex payload for a tint, or `None` for `Tint::None` (a reset carries no color).
pub const fn tint_color(tint: Tint) -> Option<&'static str> {
    match tint {
        Tint::None => None,
        Tint::Amber => Some(AMBER),
        Tint::Red => Some(RED),
        Tint::Green => Some(GREEN),
    }
}

/// Build the OSC 11 "set background" escape for a hex color (no leading `#`).
pub fn osc_set(color: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(color.len() + 8);
    bytes.extend_from_slice(b"\x1b]11;#");
    bytes.extend_from_slice(color.as_bytes());
    bytes.extend_from_slice(b"\x1b\\");
    bytes
}

/// The OSC 111 "reset background" escape — restores the terminal's configured default.
pub const fn osc_reset() -> &'static [u8] {
    b"\x1b]111\x1b\\"
}

// --- writer: detached, best-effort OSC pane-tint writes (spec 006) ------------------------

// Darwin (`sys/fcntl.h`) fcntl flag values — avoids a `libc` dependency for two constants;
// this crate is macOS-only by design (spec 006: no new dependency for the writer).
//
// These were WRONG until wave 13: they held Linux's asm-generic values (0o400 / 0o4000), which
// on Darwin mean O_NOFOLLOW and O_EXCL. The open would not have failed loudly — it would have
// requested the wrong semantics, and `write_bytes` swallows every error by design, so the
// highlight would simply have misbehaved in silence.
const O_NOCTTY: i32 = 0x0002_0000;
const O_NONBLOCK: i32 = 0x0000_0004;

/// Delay between `Pulse` frames — six frames alternate bright/dim green over ~1 s (spec 006).
const PULSE_FRAME_INTERVAL: Duration = Duration::from_millis(160);
/// Upper bound on quit-time cleanup — a wedged/closed pts must never hang the exit.
const RESET_ALL_TIMEOUT: Duration = Duration::from_millis(500);

/// Open `pts` write-only, non-blocking, and write `bytes`. Every failure (pane closed since
/// the last sweep, permission, a non-controlling process, ...) is silently dropped — a dead
/// pane is normal, the footer must never spam highlight-write errors.
fn write_bytes(pts: &str, bytes: &[u8]) {
    let Ok(mut file) = OpenOptions::new()
        .write(true)
        .custom_flags(O_NONBLOCK | O_NOCTTY)
        .open(pts)
    else {
        return;
    };
    let _ = file.write_all(bytes);
}

/// Run one write off the UI task — the blocking open/write happens on tokio's blocking pool.
async fn write_escape(pts: String, bytes: Vec<u8>) {
    let _ = tokio::task::spawn_blocking(move || write_bytes(&pts, &bytes)).await;
}

/// Apply a batch of highlight commands, each detached on its own tokio task — commands apply
/// concurrently, so a `Pulse`'s ~1 s of frame sleeps never delays a `Steady` write for another
/// pane in the same batch. Different cmds never target the same pts within one batch: the model
/// dedups per session, AND `update_highlights` suppresses a vanished session's reset when its
/// pts is reclaimed by a live row within the same sweep (pane reuse) — so per-cmd tasks never
/// race each other's writes.
///
/// ponytail: a pulse in flight isn't cancellable — a reset landing mid-pulse (reachable only via
/// a manual-refresh sweep firing <1 s after a finish) can be overwritten by the pulse's
/// remaining green frames, leaving stale green until the next transition. Known ceiling, not
/// worth a per-pts serialized writer registry unless it starts biting.
pub fn spawn_apply(cmds: Vec<HighlightCmd>) {
    for cmd in cmds {
        tokio::spawn(apply_one(cmd));
    }
}

/// Apply one highlight command. `Steady { tint: None }` writes an OSC 111 reset; `Pulse`
/// alternates bright/dim green six times (~1 s) and ends on steady dark green.
async fn apply_one(cmd: HighlightCmd) {
    match cmd {
        HighlightCmd::Steady { pts, tint } => {
            let bytes = tint_color(tint).map_or_else(|| osc_reset().to_vec(), osc_set);
            write_escape(pts, bytes).await;
        }
        HighlightCmd::Pulse { pts } => {
            const FRAMES: [&str; 6] = [
                PULSE_BRIGHT,
                GREEN,
                PULSE_BRIGHT,
                GREEN,
                PULSE_BRIGHT,
                GREEN,
            ];
            for (i, color) in FRAMES.into_iter().enumerate() {
                write_escape(pts.clone(), osc_set(color)).await;
                if i + 1 < FRAMES.len() {
                    tokio::time::sleep(PULSE_FRAME_INTERVAL).await;
                }
            }
        }
    }
}

/// Quit-time cleanup: reset every currently-tinted pane. Bounded by `RESET_ALL_TIMEOUT` — a
/// wedged or closed pts must never hang the exit.
pub async fn reset_all(pts_list: Vec<String>) {
    let write_all = async {
        for pts in pts_list {
            write_escape(pts, osc_reset().to_vec()).await;
        }
    };
    let _ = tokio::time::timeout(RESET_ALL_TIMEOUT, write_all).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_tint_table() {
        let cases = [
            (Status::NeedsAnswer, Tint::Amber),
            (Status::Waiting, Tint::Amber),
            (Status::Stalled, Tint::Red),
            (Status::Working, Tint::None),
            (Status::Idle, Tint::None),
            (Status::Shell, Tint::None),
            (Status::Unknown, Tint::None),
        ];
        for (status, want) in cases {
            assert_eq!(desired_tint(status), want, "{status:?}");
        }
    }

    #[test]
    fn tint_color_table() {
        assert_eq!(tint_color(Tint::None), None, "a reset carries no color");
        assert_eq!(tint_color(Tint::Amber), Some(AMBER));
        assert_eq!(tint_color(Tint::Red), Some(RED));
        assert_eq!(tint_color(Tint::Green), Some(GREEN));
    }

    #[test]
    fn fcntl_flags_are_the_darwin_values_not_linuxs() {
        // Regression pin (wave 13). These held Linux's asm-generic values through wave 12. On
        // Darwin those numbers name DIFFERENT flags, so the open silently requested the wrong
        // semantics — and since `write_bytes` drops every error by design, nothing would have
        // reported it. Ground truth: Darwin sys/fcntl.h.
        assert_eq!(O_NONBLOCK, 0x0000_0004, "Darwin O_NONBLOCK");
        assert_eq!(O_NOCTTY, 0x0002_0000, "Darwin O_NOCTTY");
        assert_ne!(
            O_NONBLOCK, 0o4000,
            "0o4000 is Linux's; on Darwin it is O_EXCL"
        );
        assert_ne!(
            O_NOCTTY, 0o400,
            "0o400 is Linux's; on Darwin it is O_NOFOLLOW"
        );
    }

    #[test]
    fn osc_set_exact_bytes() {
        assert_eq!(osc_set(AMBER), b"\x1b]11;#453000\x1b\\".to_vec());
        assert_eq!(osc_set(RED), b"\x1b]11;#3a0d0d\x1b\\".to_vec());
        assert_eq!(osc_set(GREEN), b"\x1b]11;#0a3512\x1b\\".to_vec());
        assert_eq!(osc_set(PULSE_BRIGHT), b"\x1b]11;#1a7a30\x1b\\".to_vec());
    }

    #[test]
    fn osc_reset_exact_bytes() {
        assert_eq!(osc_reset(), b"\x1b]111\x1b\\");
    }

    /// Manual live-verify harness against a real pts (spec 006's "escape lane" probe). Not run
    /// by `cargo test`/CI — opt in with `FLEET_PROBE_PTS=/dev/ttysNNN cargo test -- --ignored`
    /// against a scratch terminal, never a live session's surface.
    #[test]
    #[ignore = "manual probe: writes real escape sequences to a live pts"]
    fn live_probe_amber_pulse_reset() {
        let Ok(pts) = std::env::var("FLEET_PROBE_PTS") else {
            return; // opt-in only
        };
        std::fs::write(&pts, osc_set(AMBER)).expect("amber write");
        std::thread::sleep(std::time::Duration::from_millis(400));
        std::fs::write(&pts, osc_set(PULSE_BRIGHT)).expect("pulse-bright write");
        std::thread::sleep(std::time::Duration::from_millis(400));
        std::fs::write(&pts, osc_set(GREEN)).expect("pulse-settle write");
        std::thread::sleep(std::time::Duration::from_millis(400));
        std::fs::write(&pts, osc_reset()).expect("reset write");
    }
}
