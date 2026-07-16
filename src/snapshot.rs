//! `fleet snapshot` — headless one-shot: the board's rows as one JSON object on stdout.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/snapshot.rs
//! Deps:    serde/serde_json (already deps); collect, panes, board, runner
//! Tested:  inline `#[cfg(test)]` — `render_json` field shape / order / nulls (pure surface)
//!
//! Key responsibilities:
//! - Gather EXACTLY the board's rows, in the same order, via the shared `collect::collect`
//!   pipeline (never a second data path — the snapshot and the live board can't disagree).
//! - Read `focused_pane_id` from `wezterm cli list-clients` (the least-idle client) and serialize
//!   the spec-009 JSON contract with `serde_json`.
//!
//! Design constraints:
//! - Read-only over the fleet. Exit 0 on success (even 0 sessions); non-zero only on scan failure
//!   (sessions dir unreadable, or the blocking scan task crashing).
//! - No secrets: only names/counts/ids leave here (same discipline as the board).

use serde::Serialize;

use crate::board::SessionRow;
use crate::runner::Runner;
use crate::{collect, panes};

/// The spec-009 JSON document.
#[derive(Debug, Serialize)]
struct SnapshotJson {
    focused_pane_id: Option<u64>,
    sessions: Vec<SessionJson>,
}

/// One session row in the snapshot contract.
#[derive(Debug, Serialize)]
struct SessionJson {
    /// 1-based board row order.
    n: usize,
    name: String,
    /// Exact `fold::Status` variant name.
    status: &'static str,
    tokens: Option<u64>,
    ctx_pct: Option<u8>,
    /// Seconds since the transcript last appended (`SessionRow.secs_since_append`); the raw age
    /// the board's AGE column humanizes. `null` when unknown (spec 010).
    age_secs: Option<u64>,
    pane_id: Option<u64>,
    tab_index: Option<u64>,
    cwd: String,
    session_id: String,
}

/// Render the contract JSON from the focused pane + the assembled rows (pure).
fn render_json(focused_pane_id: Option<u64>, rows: &[SessionRow]) -> String {
    let sessions = rows
        .iter()
        .enumerate()
        .map(|(i, r)| SessionJson {
            n: i + 1,
            name: r.name.clone(),
            status: r.status.name(),
            tokens: r.context_tokens,
            ctx_pct: r.ctx_pct,
            age_secs: r.secs_since_append,
            pane_id: r.pane.as_ref().map(|p| p.pane_id),
            tab_index: r.pane.as_ref().map(|p| p.tab_index),
            cwd: r.cwd.clone(),
            session_id: r.session_id.clone(),
        })
        .collect();
    // Serializing our own owned data never fails; the fallback keeps this off the `unwrap` path.
    serde_json::to_string_pretty(&SnapshotJson {
        focused_pane_id,
        sessions,
    })
    .unwrap_or_else(|_| "{}".to_string())
}

/// Gather the snapshot and render it. Returns `(json, scan_ok)` — `scan_ok == false` (sessions
/// dir unreadable or the scan task crashed) means exit non-zero, exactly like `fleet doctor`.
pub async fn run(runner: &dyn Runner) -> (String, bool) {
    // Focused pane, the pane list and the process table are independent — fetch concurrently.
    let (focused, panes_result, procs_result) = tokio::join!(
        panes::focused_pane_id(runner),
        panes::list_all_panes(runner),
        crate::procsrc::fetch(runner)
    );
    let collected = tokio::task::spawn_blocking(move || {
        // Fresh caches: a one-shot has nothing to reuse across sweeps.
        let mut tails = crate::telemetry::TailCache::default();
        let mut pane_cache = crate::panes::PaneCache::default();
        collect::collect(&mut tails, &mut pane_cache, panes_result, procs_result)
    })
    .await;
    match collected {
        Ok(collected) => {
            let scan_ok = !collected.stats.dir_unreadable;
            (render_json(focused, &collected.rows), scan_ok)
        }
        // A crashed scan task must not render as a clean, empty snapshot with exit 0.
        Err(e) => (
            format!("{{\"error\":\"snapshot scan task failed: {e}\"}}"),
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::MatchedPane;
    use crate::fold::Status;
    use serde_json::Value;

    fn matched_row(
        id: &str,
        status: Status,
        name: &str,
        tokens: Option<u64>,
        ctx_pct: Option<u8>,
        pane_id: u64,
        tab_index: u64,
    ) -> SessionRow {
        SessionRow {
            session_id: id.to_string(),
            name: name.to_string(),
            account: Some("alpha".to_string()),
            status,
            cwd: "/tui/fleetops".to_string(),
            context_tokens: tokens,
            ctx_pct,
            secs_since_append: Some(3),
            pane: Some(MatchedPane {
                socket: String::new(),
                tab_id: 3,
                pane_id,
                tab_index,
            }),
            pane_ambiguous: false,
            pts: None,
        }
    }

    fn unmatched_row(id: &str, status: Status, name: &str) -> SessionRow {
        SessionRow {
            session_id: id.to_string(),
            name: name.to_string(),
            account: None,
            status,
            cwd: "/home/user/x".to_string(),
            context_tokens: None,
            ctx_pct: None,
            secs_since_append: None,
            pane: None,
            pane_ambiguous: false,
            pts: None,
        }
    }

    #[test]
    fn render_json_matches_the_contract_shape_order_and_nulls() {
        let rows = vec![
            matched_row(
                "s1",
                Status::NeedsAnswer,
                "Pick an option",
                Some(120_000),
                Some(60),
                47,
                1,
            ),
            unmatched_row("s2", Status::Working, "young session"),
        ];
        let json = render_json(Some(21), &rows);
        let v: Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(v["focused_pane_id"], 21);
        let s = &v["sessions"];
        assert_eq!(s.as_array().expect("array").len(), 2);

        // Row 0: matched, everything present, 1-based n.
        assert_eq!(s[0]["n"], 1);
        assert_eq!(s[0]["name"], "Pick an option");
        assert_eq!(s[0]["status"], "NeedsAnswer");
        assert_eq!(s[0]["tokens"], 120_000);
        assert_eq!(s[0]["ctx_pct"], 60);
        assert_eq!(s[0]["age_secs"], 3, "age_secs = secs_since_append");
        assert_eq!(s[0]["pane_id"], 47);
        assert_eq!(s[0]["tab_index"], 1);
        assert_eq!(s[0]["cwd"], "/tui/fleetops");
        assert_eq!(s[0]["session_id"], "s1");

        // Row 1: unmatched → pane_id/tab_index/tokens/ctx_pct/age_secs all null; n advances.
        assert_eq!(s[1]["n"], 2);
        assert_eq!(s[1]["status"], "Working");
        assert!(s[1]["tokens"].is_null());
        assert!(s[1]["ctx_pct"].is_null());
        assert!(
            s[1]["age_secs"].is_null(),
            "no age when secs_since_append is None"
        );
        assert!(s[1]["pane_id"].is_null());
        assert!(s[1]["tab_index"].is_null());
    }

    #[test]
    fn render_json_zero_sessions_and_no_focused_pane_is_valid() {
        let json = render_json(None, &[]);
        let v: Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(v["focused_pane_id"].is_null());
        assert_eq!(v["sessions"].as_array().expect("array").len(), 0);
    }
}
