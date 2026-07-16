//! `fleet snapshot` — headless one-shot: the board's rows as one JSON object on stdout.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/snapshot.rs
//! Deps:    serde/serde_json (already deps); collect, cmux, board, runner
//! Tested:  inline `#[cfg(test)]` — `render_json` field shape / order / nulls (pure surface)
//!
//! Key responsibilities:
//! - Gather EXACTLY the board's rows, in the same order, via the shared `collect::collect`
//!   pipeline (never a second data path — the snapshot and the live board can't disagree).
//! - Read the focused surface from cmux (AppleScript) and serialize
//!   the spec-009 JSON contract with `serde_json`.
//!
//! Design constraints:
//! - Read-only over the fleet. Exit 0 on success (even 0 sessions); non-zero only on scan failure
//!   (sessions dir unreadable, or the blocking scan task crashing).
//! - No secrets: only names/counts/ids leave here (same discipline as the board).

use serde::Serialize;

use crate::board::SessionRow;
use crate::runner::Runner;
use crate::{cmux, collect};

/// The spec-009 JSON document.
#[derive(Debug, Serialize)]
struct SnapshotJson {
    focused_surface_id: Option<String>,
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
    surface_id: Option<String>,
    tab_index: Option<u32>,
    cwd: String,
    session_id: String,
}

/// Render the contract JSON from the focused surface + the assembled rows (pure).
fn render_json(focused_surface_id: Option<String>, rows: &[SessionRow]) -> String {
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
            surface_id: r.pane.as_ref().map(|p| p.surface_id.clone()),
            tab_index: r.pane.as_ref().map(|p| p.tab_index),
            cwd: r.cwd.clone(),
            session_id: r.session_id.clone(),
        })
        .collect();
    // Serializing our own owned data never fails; the fallback keeps this off the `unwrap` path.
    serde_json::to_string_pretty(&SnapshotJson {
        focused_surface_id,
        sessions,
    })
    .unwrap_or_else(|_| "{}".to_string())
}

/// Gather the snapshot and render it. Returns `(json, scan_ok)` — `scan_ok == false` (sessions
/// dir unreadable or the scan task crashed) means exit non-zero, exactly like `fleet doctor`.
pub async fn run(runner: &dyn Runner) -> (String, bool) {
    // The focused surface, the topology and the process table are independent — fetch
    // concurrently.
    let (focused, surfaces_result, procs_result) = tokio::join!(
        cmux::focused_surface_id(runner),
        cmux::list(runner),
        crate::procsrc::fetch(runner)
    );
    // The Codex lane needs the process table, so it follows it.
    let codex_procs = match &procs_result {
        Ok(table) => crate::codex::fetch(runner, table).await.unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let collected = tokio::task::spawn_blocking(move || {
        // Fresh caches: a one-shot has nothing to reuse across sweeps.
        let mut tails = crate::telemetry::TailCache::default();
        let mut surface_cache = crate::cmux::SurfaceCache::default();
        collect::collect(
            &mut tails,
            &mut surface_cache,
            surfaces_result,
            procs_result,
            &codex_procs,
        )
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
        surface_id: &str,
        tab_index: u32,
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
            stream: Some("gtm-studio".to_string()),
            branch: Some("main".to_string()),
            pane: Some(MatchedPane {
                surface_id: surface_id.to_string(),
                window_index: 1,
                tab_index,
                stream: "a-stream".to_string(),
            }),
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
            stream: None,
            branch: None,
            pane: None,
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
                "uuid-47",
                1,
            ),
            unmatched_row("s2", Status::Working, "young session"),
        ];
        let json = render_json(Some("uuid-focused".to_string()), &rows);
        let v: Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(v["focused_surface_id"], "uuid-focused");
        let s = &v["sessions"];
        assert_eq!(s.as_array().expect("array").len(), 2);

        // Row 0: matched, everything present, 1-based n.
        assert_eq!(s[0]["n"], 1);
        assert_eq!(s[0]["name"], "Pick an option");
        assert_eq!(s[0]["status"], "NeedsAnswer");
        assert_eq!(s[0]["tokens"], 120_000);
        assert_eq!(s[0]["ctx_pct"], 60);
        assert_eq!(s[0]["age_secs"], 3, "age_secs = secs_since_append");
        assert_eq!(s[0]["surface_id"], "uuid-47");
        assert_eq!(s[0]["tab_index"], 1);
        assert_eq!(s[0]["cwd"], "/tui/fleetops");
        assert_eq!(s[0]["session_id"], "s1");

        // Row 1: unmatched → surface_id/tab_index/tokens/ctx_pct/age_secs all null; n advances.
        assert_eq!(s[1]["n"], 2);
        assert_eq!(s[1]["status"], "Working");
        assert!(s[1]["tokens"].is_null());
        assert!(s[1]["ctx_pct"].is_null());
        assert!(
            s[1]["age_secs"].is_null(),
            "no age when secs_since_append is None"
        );
        assert!(s[1]["surface_id"].is_null());
        assert!(s[1]["tab_index"].is_null());
    }

    #[test]
    fn render_json_zero_sessions_and_no_focused_surface_is_valid() {
        let json = render_json(None, &[]);
        let v: Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(v["focused_surface_id"].is_null());
        assert_eq!(v["sessions"].as_array().expect("array").len(), 0);
    }
}
