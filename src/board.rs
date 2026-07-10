//! Board assembly: join discovery + telemetry + panes into sorted `SessionRow`s — pure.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/board.rs
//! Deps:    discovery, telemetry, fold, panes (types only)
//! Tested:  inline `#[cfg(test)]` — pane match table, assembly ordering, name preference
//!
//! Key responsibilities:
//! - `match_pane`: session (cwd, name) → wezterm pane, tie-broken by title, ambiguity surfaced.
//! - `assemble`: fold each session's status, prefer ai-title over the derived name, sort by
//!   attention bucket then name.
//!
//! Design constraints:
//! - Pure — the sensor task calls this with data already in hand; no I/O, no clocks.
//! - Ambiguous pane matches are marked, never guessed silently (dossier pre-mortem #4).

use crate::discovery::LiveSession;
use crate::fold::{self, Status};
use crate::panes::PaneRow;
use crate::telemetry::Telemetry;

/// One board row — a live session with everything the view renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    /// Session UUID — identity (selection survives refreshes on it).
    pub session_id: String,
    /// Semantic name: transcript ai-title if present, else the native derived name.
    pub name: String,
    /// `CLAUDE_ACCOUNT`, if attributed.
    pub account: Option<String>,
    /// Folded status.
    pub status: Status,
    /// Session working directory.
    pub cwd: String,
    /// Context tokens (statusline recipe); `None` = no transcript yet.
    pub context_tokens: Option<u64>,
    /// Seconds since the transcript last grew.
    pub secs_since_append: Option<u64>,
    /// Matched wezterm pane `(tab_id, pane_id)` — the jump target.
    pub pane: Option<(u64, u64)>,
    /// More than one pane matched and the title tie-break failed.
    pub pane_ambiguous: bool,
}

/// Match a session to a wezterm pane. Priority (spec 005):
/// 1. exact — the session's own `WEZTERM_PANE` (WSLENV-forwarded), if that pane still exists;
/// 2. title (verified live: WSL panes report the Windows cwd `file:///C:/Users/user/` — OSC7
///    cwd never crosses from WSL, only titles do);
/// 3. cwd, for the rare pane whose cwd did cross. Returns `(pane, ambiguous)`.
pub fn match_pane(
    env_pane: Option<u64>,
    cwd: &str,
    names: &[&str],
    panes: &[PaneRow],
) -> (Option<(u64, u64)>, bool) {
    if let Some(id) = env_pane {
        if let Some(p) = panes.iter().find(|p| p.pane_id == id) {
            return (Some((p.tab_id, p.pane_id)), false); // exact identity, never ambiguous
        }
        // env pane gone from the list (pane closed / other window) — fall through.
    }
    let by_title: Vec<&PaneRow> = panes
        .iter()
        .filter(|p| names.iter().any(|n| !n.is_empty() && p.name == *n))
        .collect();
    match by_title.as_slice() {
        [only] => return (Some((only.tab_id, only.pane_id)), false),
        [_, ..] => return (None, true),
        [] => {}
    }
    let by_cwd: Vec<&PaneRow> = panes.iter().filter(|p| p.cwd == cwd).collect();
    match by_cwd.as_slice() {
        [] => (None, false),
        [only] => (Some((only.tab_id, only.pane_id)), false),
        [_, ..] => (None, true),
    }
}

/// Join sessions with their telemetry (parallel slice, same order) and the pane list.
/// Output is sorted: attention buckets first, then by name.
pub fn assemble(
    sessions: &[LiveSession],
    telemetry: &[Telemetry],
    panes: &[PaneRow],
) -> Vec<SessionRow> {
    let mut rows: Vec<SessionRow> = sessions
        .iter()
        .zip(telemetry)
        .map(|(session, tel)| {
            let facts = tel.facts.clone().unwrap_or_default();
            let status = fold::status(
                &session.file.status,
                facts.pending_question,
                tel.secs_since_append,
            );
            let name = facts
                .ai_title
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| session.file.name.clone());
            // Try both the shown name and the native name — the pane title may carry either.
            let (pane, pane_ambiguous) = match_pane(
                session.wezterm_pane,
                &session.file.cwd,
                &[&name, &session.file.name],
                panes,
            );
            SessionRow {
                session_id: session.file.session_id.clone(),
                name,
                account: session.account.clone(),
                status,
                cwd: session.file.cwd.clone(),
                context_tokens: facts.context_tokens,
                secs_since_append: tel.secs_since_append,
                pane,
                pane_ambiguous,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        fold::sort_key(a.status)
            .cmp(&fold::sort_key(b.status))
            .then_with(|| a.name.cmp(&b.name))
    });
    rows
}

/// Humanized age: `7s`, `4m`, `2h`, `3d`.
pub fn format_age(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3_599 => format!("{}m", secs / 60),
        3_600..=86_399 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{NativeStatus, SessionFile};
    use crate::panes::PaneStatus;
    use crate::telemetry::TailFacts;

    fn pane(pane_id: u64, cwd: &str, name: &str) -> PaneRow {
        PaneRow {
            pane_id,
            tab_id: 1,
            status: PaneStatus::Working,
            name: name.to_string(),
            cwd: cwd.to_string(),
            is_active: false,
        }
    }

    fn session(id: &str, cwd: &str, name: &str, status: NativeStatus) -> LiveSession {
        LiveSession {
            file: SessionFile {
                pid: 1,
                session_id: id.to_string(),
                cwd: cwd.to_string(),
                proc_start: "1".to_string(),
                name: name.to_string(),
                status,
                updated_at_ms: 0,
                version: None,
            },
            account: Some("golf-acct".to_string()),
            wezterm_pane: None,
        }
    }

    fn telemetry(facts: Option<TailFacts>, age: Option<u64>) -> Telemetry {
        Telemetry {
            facts,
            secs_since_append: age,
        }
    }

    #[test]
    fn match_pane_table() {
        let panes = [
            pane(1, "/a", "one"),
            pane(2, "/b", "two"),
            pane(3, "/b", "three"),
            pane(4, "C:/Users/user", "dupe"),
            pane(5, "C:/Users/user", "dupe"),
        ];
        // exact env pane beats everything, even a duplicate title situation
        assert_eq!(
            match_pane(Some(4), "/z", &["dupe"], &panes),
            (Some((1, 4)), false)
        );
        // env pane no longer in the list → falls through to title
        assert_eq!(
            match_pane(Some(99), "/z", &["three"], &panes),
            (Some((1, 3)), false)
        );
        // title match is primary — cwd wrong (the WSL reality) but title unique
        assert_eq!(
            match_pane(None, "/z", &["three"], &panes),
            (Some((1, 3)), false)
        );
        // second name (native) matches when the first (ai-title) doesn't
        assert_eq!(
            match_pane(None, "/z", &["no", "two"], &panes),
            (Some((1, 2)), false)
        );
        // duplicate titles → ambiguous, never guessed
        assert_eq!(match_pane(None, "/z", &["dupe"], &panes), (None, true));
        // empty names never match empty-titled panes
        assert_eq!(match_pane(None, "/z", &[""], &panes), (None, false));
        // cwd fallback: unique
        assert_eq!(
            match_pane(None, "/a", &["nomatch"], &panes),
            (Some((1, 1)), false)
        );
        // cwd fallback: ambiguous
        assert_eq!(match_pane(None, "/b", &["nomatch"], &panes), (None, true));
        // nothing matches
        assert_eq!(match_pane(None, "/z", &["x"], &panes), (None, false));
    }

    #[test]
    fn assemble_prefers_ai_title_and_sorts_attention_first() {
        let sessions = [
            session("s-idle", "/a", "idle native", NativeStatus::Idle),
            session("s-ask", "/b", "ask native", NativeStatus::Busy),
            session("s-work", "/c", "work native", NativeStatus::Busy),
        ];
        let tel = [
            telemetry(Some(TailFacts::default()), Some(10)),
            telemetry(
                Some(TailFacts {
                    pending_question: true,
                    ai_title: Some("Pick an option".to_string()),
                    context_tokens: Some(120_000),
                }),
                Some(5),
            ),
            telemetry(Some(TailFacts::default()), Some(10)),
        ];
        let rows = assemble(&sessions, &tel, &[pane(7, "/b", "Pick an option")]);
        assert_eq!(rows[0].session_id, "s-ask", "NeedsAnswer sorts first");
        assert_eq!(rows[0].status, Status::NeedsAnswer);
        assert_eq!(rows[0].name, "Pick an option", "ai-title wins");
        assert_eq!(rows[0].pane, Some((1, 7)));
        assert_eq!(rows[0].context_tokens, Some(120_000));
        assert_eq!(rows[1].status, Status::Working);
        assert_eq!(rows[2].status, Status::Idle);
    }

    #[test]
    fn assemble_without_transcript_uses_native_name_and_no_tokens() {
        let sessions = [session("s1", "/a", "native", NativeStatus::Busy)];
        let rows = assemble(&sessions, &[Telemetry::default()], &[]);
        assert_eq!(rows[0].name, "native");
        assert_eq!(rows[0].context_tokens, None);
        assert_eq!(
            rows[0].status,
            Status::Working,
            "no transcript = young, not stalled"
        );
    }

    #[test]
    fn format_age_table() {
        let cases = [
            (0, "0s"),
            (59, "59s"),
            (60, "1m"),
            (3_599, "59m"),
            (7_200, "2h"),
            (90_000, "1d"),
        ];
        for (secs, want) in cases {
            assert_eq!(format_age(secs), want, "secs={secs}");
        }
    }
}
