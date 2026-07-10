//! TUI state machine: `App` + `update(Msg)` — pure, no I/O.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/tui/model.rs
//! Deps:    crate::board (SessionRow), crate::discovery (ScanStats)
//! Tested:  inline `#[cfg(test)]` unit tests (selection, refresh, quit, errors)
//!
//! Key responsibilities:
//! - Hold board state: session rows, selection (keyed by sessionId), refresh age, sensor errors.
//! - Fold every `Msg` into the next state; expose effect requests (jump/refresh) as flags
//!   the event loop drains — the model itself never performs them.
//!
//! Design constraints:
//! - Pure: no clocks, no subprocess, no terminal. Age is counted in ticks fed by the loop.
//! - Selection is keyed by `session_id`, not row index — a refresh must not steal the cursor.

use crate::board::{MatchedPane, SessionRow};
use crate::discovery::ScanStats;

/// User intent, mapped from keys (see `keys.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Move selection up one row.
    Up,
    /// Move selection down one row.
    Down,
    /// Jump to the selected session's pane.
    Jump,
    /// Force a sensor refresh now.
    Refresh,
    /// Quit the board.
    Quit,
}

/// One full sensor sweep: assembled rows + scan tallies + a non-fatal lane error, if any.
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Monotone sweep number, assigned at spawn time. Sweeps run concurrently and finish out
    /// of order (a wezterm timeout takes 5 s, the next poll fires at 2 s) — a snapshot older
    /// than the newest applied one is dropped, never shown (stale-shown-as-live class).
    pub seq: u64,
    /// Assembled, sorted session rows.
    pub rows: Vec<SessionRow>,
    /// Discovery tallies (footer + doctor).
    pub stats: ScanStats,
    /// A degraded lane (e.g. wezterm unreachable) — rows are still valid.
    pub lane_error: Option<String>,
}

/// Everything the event loop can feed the model.
#[derive(Debug)]
pub enum Msg {
    /// A completed sensor sweep.
    Snapshot(Box<Snapshot>),
    /// A whole-sweep or jump failure to surface in the footer.
    Error(String),
    /// A key mapped to an action.
    Key(Action),
    /// One second elapsed.
    Tick,
}

/// The board state. `view` reads it; only `update` writes it.
#[derive(Debug, Default)]
pub struct App {
    /// Session rows, sorted by attention bucket then name.
    pub rows: Vec<SessionRow>,
    /// Latest scan tallies.
    pub stats: ScanStats,
    /// Selected session's id (survives refreshes); `None` when the board is empty.
    pub selected: Option<String>,
    /// Seconds since the last successful sweep.
    pub refresh_age_secs: u64,
    /// Last sensor/jump error, shown in the footer until the next clean sweep.
    pub error: Option<String>,
    /// Loop flag: exit.
    pub should_quit: bool,
    /// Effect request: jump to this pane (drained by the loop) — activate the tab too (pane
    /// activation alone doesn't bring the tab forward), against the pane's own instance.
    pub pending_jump: Option<MatchedPane>,
    /// Effect request: poll the sensors now (drained by the loop).
    pub refresh_requested: bool,
    /// Seq of the newest applied snapshot; older arrivals are dropped.
    applied_seq: u64,
}

impl App {
    /// Index of the selected row, if it still exists.
    pub fn selected_index(&self) -> Option<usize> {
        let id = self.selected.as_deref()?;
        self.rows.iter().position(|r| r.session_id == id)
    }

    /// Fold one message into the next state.
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Snapshot(snapshot) => {
                if snapshot.seq < self.applied_seq {
                    return; // an older sweep finished late — never let it stomp fresh data
                }
                self.applied_seq = snapshot.seq;
                let old_index = self.selected_index();
                self.rows = snapshot.rows;
                self.stats = snapshot.stats;
                self.refresh_age_secs = 0;
                self.error = snapshot.lane_error;
                self.reselect(old_index);
            }
            Msg::Error(e) => self.error = Some(e),
            Msg::Tick => self.refresh_age_secs = self.refresh_age_secs.saturating_add(1),
            Msg::Key(action) => self.apply(action),
        }
    }

    /// Keep the selection on the same session if it survived; otherwise clamp to the nearest row.
    fn reselect(&mut self, old_index: Option<usize>) {
        if self.rows.is_empty() {
            self.selected = None;
            return;
        }
        let still_there = self
            .selected
            .as_deref()
            .is_some_and(|id| self.rows.iter().any(|r| r.session_id == id));
        if still_there {
            return;
        }
        let index = old_index.unwrap_or(0).min(self.rows.len() - 1);
        self.selected = Some(self.rows[index].session_id.clone());
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Refresh => self.refresh_requested = true,
            Action::Jump => {
                let Some(index) = self.selected_index() else {
                    return;
                };
                let row = &self.rows[index];
                match &row.pane {
                    Some(p) => self.pending_jump = Some(p.clone()),
                    None if row.pane_ambiguous => {
                        self.error = Some(format!("jump: several panes match '{}'", row.name));
                    }
                    None => {
                        self.error = Some(format!("jump: no pane matched '{}'", row.name));
                    }
                }
            }
            Action::Up => self.move_selection(-1),
            Action::Down => self.move_selection(1),
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let current = self.selected_index().unwrap_or(0);
        let last = self.rows.len() - 1;
        let next = current.saturating_add_signed(delta).min(last);
        self.selected = Some(self.rows[next].session_id.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::MatchedPane;
    use crate::fold::Status;

    fn row(id: &str, pane: Option<(u64, u64)>, ambiguous: bool) -> SessionRow {
        SessionRow {
            session_id: id.to_string(),
            name: format!("session {id}"),
            account: Some("golf-acct".to_string()),
            status: Status::Working,
            cwd: "/tui/x".to_string(),
            context_tokens: Some(50_000),
            secs_since_append: Some(3),
            pane: pane.map(|(tab_id, pane_id)| MatchedPane {
                socket: String::new(),
                tab_id,
                pane_id,
                tab_index: 1,
            }),
            pane_ambiguous: ambiguous,
        }
    }

    fn snapshot(rows: Vec<SessionRow>) -> Msg {
        Msg::Snapshot(Box::new(Snapshot {
            rows,
            ..Snapshot::default()
        }))
    }

    fn app_with(ids: &[&str]) -> App {
        let mut app = App::default();
        app.update(snapshot(
            ids.iter().map(|id| row(id, Some((1, 9)), false)).collect(),
        ));
        app
    }

    #[test]
    fn first_snapshot_selects_first_row() {
        let app = app_with(&["a", "b", "c"]);
        assert_eq!(app.selected.as_deref(), Some("a"));
    }

    #[test]
    fn selection_moves_and_clamps_at_edges() {
        let mut app = app_with(&["a", "b", "c"]);
        app.update(Msg::Key(Action::Up));
        assert_eq!(app.selected.as_deref(), Some("a"), "clamped at top");
        for _ in 0..5 {
            app.update(Msg::Key(Action::Down));
        }
        assert_eq!(app.selected.as_deref(), Some("c"), "clamped at bottom");
    }

    #[test]
    fn refresh_preserves_selection_by_session_id() {
        let mut app = app_with(&["a", "b", "c"]);
        app.update(Msg::Key(Action::Down)); // select b
                                            // Rows resort: b moves to index 2.
        app.update(snapshot(vec![
            row("c", None, false),
            row("a", None, false),
            row("b", None, false),
        ]));
        assert_eq!(app.selected.as_deref(), Some("b"));
        assert_eq!(app.selected_index(), Some(2));
    }

    #[test]
    fn vanished_selection_clamps_to_nearest_row() {
        let mut app = app_with(&["a", "b", "c"]);
        app.update(Msg::Key(Action::Down));
        app.update(Msg::Key(Action::Down)); // select c (index 2)
        app.update(snapshot(vec![row("a", None, false), row("b", None, false)]));
        assert_eq!(
            app.selected.as_deref(),
            Some("b"),
            "clamped to last remaining row"
        );
    }

    #[test]
    fn empty_board_clears_selection() {
        let mut app = app_with(&["a"]);
        app.update(snapshot(Vec::new()));
        assert_eq!(app.selected, None);
        app.update(Msg::Key(Action::Jump));
        assert_eq!(app.pending_jump, None);
    }

    #[test]
    fn jump_uses_matched_tab_and_pane() {
        let mut app = app_with(&["a"]);
        app.update(Msg::Key(Action::Jump));
        assert_eq!(
            app.pending_jump.as_ref().map(|p| (p.tab_id, p.pane_id)),
            Some((1, 9)),
            "tab id needed to bring the tab forward"
        );
    }

    #[test]
    fn jump_without_pane_match_reports_not_crashes() {
        let mut app = App::default();
        app.update(snapshot(vec![row("a", None, false)]));
        app.update(Msg::Key(Action::Jump));
        assert_eq!(app.pending_jump, None);
        assert!(app
            .error
            .as_deref()
            .is_some_and(|e| e.contains("no pane matched")));

        let mut app = App::default();
        app.update(snapshot(vec![row("a", None, true)]));
        app.update(Msg::Key(Action::Jump));
        assert!(app
            .error
            .as_deref()
            .is_some_and(|e| e.contains("several panes")));
    }

    #[test]
    fn clean_snapshot_resets_age_and_error_degraded_lane_error_sticks() {
        let mut app = app_with(&["a"]);
        app.update(Msg::Tick);
        app.update(Msg::Error("boom".into()));
        app.update(snapshot(vec![row("a", None, false)]));
        assert_eq!(app.refresh_age_secs, 0);
        assert_eq!(app.error, None);

        app.update(Msg::Snapshot(Box::new(Snapshot {
            rows: vec![row("a", None, false)],
            lane_error: Some("wezterm.exe: timed out after 5s".to_string()),
            ..Snapshot::default()
        })));
        assert!(app
            .error
            .as_deref()
            .is_some_and(|e| e.contains("timed out")));
    }

    #[test]
    fn late_older_sweep_is_dropped() {
        let mut app = App::default();
        app.update(Msg::Snapshot(Box::new(Snapshot {
            seq: 2,
            rows: vec![row("fresh", Some((1, 9)), false)],
            ..Snapshot::default()
        })));
        // An older sweep (seq 1) finishes late — with stale rows and a stale lane error.
        app.update(Msg::Snapshot(Box::new(Snapshot {
            seq: 1,
            rows: vec![],
            lane_error: Some("wezterm.exe: timed out after 5s".to_string()),
            ..Snapshot::default()
        })));
        assert_eq!(app.rows.len(), 1, "fresh rows kept");
        assert_eq!(app.error, None, "stale lane error never shown");
    }

    #[test]
    fn quit_and_refresh_flags() {
        let mut app = app_with(&["a"]);
        app.update(Msg::Key(Action::Refresh));
        assert!(app.refresh_requested);
        app.update(Msg::Key(Action::Quit));
        assert!(app.should_quit);
    }
}
