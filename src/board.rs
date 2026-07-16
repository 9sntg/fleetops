//! Board assembly: join discovery + telemetry + panes into sorted `SessionRow`s — pure.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/board.rs
//! Deps:    discovery, telemetry, fold, cmux (types only)
//! Tested:  inline `#[cfg(test)]` — surface match table, assembly ordering, name preference,
//!          pts flowing to the row only when a surface matched
//!
//! Key responsibilities:
//! - `match_surface`: session → its cmux surface, by exact id.
//! - `assemble`: fold each session's status, prefer ai-title over the derived name, sort by
//!   attention bucket then name.
//!
//! Design constraints:
//! - Pure — the sensor task calls this with data already in hand; no I/O, no clocks.
//! - The match is EXACT or absent: a session's `CMUX_SURFACE_ID` names one surface, and surface
//!   ids are unique UUIDs. Wave 12 retired wezterm's title/cwd tie-break tiers and with them the
//!   `ambiguous` outcome — with exact identity there is nothing left to guess (dossier
//!   pre-mortem #4 is satisfied by construction rather than by flagging).

use crate::cmux::Surface;
use crate::discovery::LiveSession;
use crate::fold::{self, Status};
use crate::telemetry::{self, Telemetry};

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
    /// Context percent used — Claude: `telemetry::ctx_used_pct`'s recipe; Codex:
    /// `total * 100 / model_context_window` (spec 008 ctx% seam). `None` = no telemetry yet.
    pub ctx_pct: Option<u8>,
    /// Seconds since the transcript last grew.
    pub secs_since_append: Option<u64>,
    /// Matched cmux surface — the jump target.
    pub pane: Option<MatchedPane>,
    /// The session's pts, carried through only when it renders in a cmux surface — the
    /// highlight write-target guard lives here, not in the writer (wave 6, spec 006).
    pub pts: Option<String>,
}

/// The cmux surface a session resolved to: the id for the jump, tab position for the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedPane {
    /// The surface UUID — the jump target (`cmux::focus`).
    pub surface_id: String,
    /// 1-based window position.
    pub window_index: u32,
    /// 1-based tab (workspace) position within its window; emitted by `fleet snapshot` for
    /// automation and rendered in the board's PANE column.
    pub tab_index: u32,
}

impl MatchedPane {
    fn from_surface(s: &Surface) -> Self {
        Self {
            surface_id: s.id.clone(),
            window_index: s.window_index,
            tab_index: s.tab_index,
        }
    }
}

/// Match a session to its cmux surface by exact id (spec 012).
///
/// The session's own `CMUX_SURFACE_ID` (read from its environment) names exactly one surface,
/// and surface ids are unique UUIDs — so this is identity, not inference. A session with no
/// surface id isn't running under cmux; a surface id absent from the list means the surface
/// closed since the sweep. Both are simply "no match", never a guess.
pub fn match_surface(surface_id: Option<&str>, surfaces: &[Surface]) -> Option<MatchedPane> {
    let wanted = surface_id?;
    surfaces
        .iter()
        .find(|s| s.id == wanted)
        .map(MatchedPane::from_surface)
}

/// Join sessions with their telemetry (parallel slice, same order) and the cmux surface list.
/// Output is sorted: attention buckets first, then by name.
pub fn assemble(
    sessions: &[LiveSession],
    telemetry: &[Telemetry],
    surfaces: &[Surface],
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
            let pane = match_surface(session.surface_id.as_deref(), surfaces);
            SessionRow {
                session_id: session.file.session_id.clone(),
                name,
                account: session.account.clone(),
                status,
                cwd: session.file.cwd.clone(),
                context_tokens: facts.context_tokens,
                ctx_pct: facts
                    .context_tokens
                    .map(|t| clamp_pct_u8(telemetry::ctx_used_pct(t))),
                secs_since_append: tel.secs_since_append,
                pane,
                // The highlight write-target guard: a session is only ever highlightable when
                // it renders in a cmux surface (spec 006, retargeted in spec 012).
                pts: if session.surface_id.is_some() {
                    session.pts.clone()
                } else {
                    None
                },
            }
        })
        .collect();
    sort_rows(&mut rows);
    rows
}

/// Clamp a ctx% (0..=100+ from `telemetry::ctx_used_pct`, unbounded on hostile input) into the
/// row's `u8` field — never panics on an absurd usage value (tolerant-parser invariant).
fn clamp_pct_u8(pct: u64) -> u8 {
    u8::try_from(pct.min(u64::from(u8::MAX))).unwrap_or(u8::MAX)
}

/// Sort assembled rows: attention buckets first, then by name — extracted so a sweep can
/// concatenate Claude + Codex rows and sort once (spec 008).
pub fn sort_rows(rows: &mut [SessionRow]) {
    rows.sort_by(|a, b| {
        fold::sort_key(a.status)
            .cmp(&fold::sort_key(b.status))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// Last path segment for display: `/home/user/project-a` → `project-a`; `/` → `/`.
pub fn dir_name(cwd: &str) -> &str {
    cwd.rsplit('/').find(|s| !s.is_empty()).unwrap_or("/")
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
    use crate::telemetry::TailFacts;

    fn surface(id: &str, tab_index: u32, cwd: &str) -> Surface {
        Surface {
            id: id.to_string(),
            window_index: 1,
            tab_index,
            cwd: cwd.to_string(),
        }
    }

    fn matched(id: &str, tab_index: u32) -> MatchedPane {
        MatchedPane {
            surface_id: id.to_string(),
            window_index: 1,
            tab_index,
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
            account: Some("alpha".to_string()),
            surface_id: None,
            pts: None,
        }
    }

    /// A session running inside a cmux surface (i.e. one that CAN be jumped to).
    fn session_in(id: &str, cwd: &str, name: &str, status: NativeStatus, sid: &str) -> LiveSession {
        LiveSession {
            surface_id: Some(sid.to_string()),
            pts: Some("/dev/ttys003".to_string()),
            ..session(id, cwd, name, status)
        }
    }

    fn telemetry(facts: Option<TailFacts>, age: Option<u64>) -> Telemetry {
        Telemetry {
            facts,
            secs_since_append: age,
        }
    }

    #[test]
    fn match_surface_table() {
        let surfaces = [
            surface("uuid-a", 1, "/a"),
            surface("uuid-b", 2, "/b"),
            surface("uuid-c", 3, "/b"), // same cwd as b — irrelevant to an id match
        ];
        // Exact identity.
        assert_eq!(
            match_surface(Some("uuid-b"), &surfaces),
            Some(matched("uuid-b", 2))
        );
        // A session with no surface id isn't under cmux.
        assert_eq!(match_surface(None, &surfaces), None);
        // A surface that closed since the sweep → no match, not a guess.
        assert_eq!(match_surface(Some("uuid-gone"), &surfaces), None);
        // Empty list.
        assert_eq!(match_surface(Some("uuid-a"), &[]), None);
    }

    #[test]
    fn a_shared_cwd_can_never_cause_a_mismatch() {
        // The wezterm lane fell back to cwd and had to report ambiguity when two panes shared
        // one. Identity is by UUID now, so a shared cwd is simply not a factor (spec 012).
        let surfaces = [surface("uuid-b", 2, "/same"), surface("uuid-c", 3, "/same")];
        assert_eq!(
            match_surface(Some("uuid-c"), &surfaces),
            Some(matched("uuid-c", 3)),
            "the id decides; the duplicate cwd is not consulted"
        );
    }

    #[test]
    fn assemble_falls_back_to_native_name_on_empty_ai_title() {
        // A transcript can carry an empty ai-title — the native name must win over "".
        let sessions = [session_in(
            "s1",
            "/a",
            "native",
            NativeStatus::Busy,
            "uuid-3",
        )];
        let tel = [telemetry(
            Some(TailFacts {
                ai_title: Some(String::new()),
                ..TailFacts::default()
            }),
            Some(1),
        )];
        let rows = assemble(&sessions, &tel, &[surface("uuid-3", 3, "/z")]);
        assert_eq!(rows[0].name, "native");
        assert_eq!(
            rows[0].pane,
            Some(matched("uuid-3", 3)),
            "the surface matches on id, independent of the name"
        );
    }

    #[test]
    fn dir_name_table() {
        let cases = [
            ("/home/user/project-a", "project-a"),
            ("/tui/fleetops", "fleetops"),
            ("/tui", "tui"),
            ("/", "/"),
            ("", "/"),
        ];
        for (cwd, want) in cases {
            assert_eq!(dir_name(cwd), want, "cwd {cwd:?}");
        }
    }

    #[test]
    fn assemble_prefers_ai_title_and_sorts_attention_first() {
        let sessions = [
            session("s-idle", "/a", "idle native", NativeStatus::Idle),
            session_in("s-ask", "/b", "ask native", NativeStatus::Busy, "uuid-7"),
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
        let rows = assemble(&sessions, &tel, &[surface("uuid-7", 7, "/b")]);
        assert_eq!(rows[0].session_id, "s-ask", "NeedsAnswer sorts first");
        assert_eq!(rows[0].status, Status::NeedsAnswer);
        assert_eq!(rows[0].name, "Pick an option", "ai-title wins");
        assert_eq!(rows[0].pane, Some(matched("uuid-7", 7)));
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

    #[test]
    fn pts_flows_to_the_row_only_when_the_session_is_in_a_cmux_surface() {
        let in_cmux = LiveSession {
            surface_id: Some("uuid-4".to_string()),
            pts: Some("/dev/ttys002".to_string()),
            ..session("s1", "/a", "one", NativeStatus::Busy)
        };
        let outside_cmux = LiveSession {
            surface_id: None,
            pts: Some("/dev/ttys002".to_string()),
            ..session("s2", "/b", "two", NativeStatus::Busy)
        };
        let tel = [Telemetry::default(), Telemetry::default()];
        let rows = assemble(&[in_cmux, outside_cmux], &tel, &[]);
        let pts_of = |id: &str| {
            rows.iter()
                .find(|r| r.session_id == id)
                .unwrap()
                .pts
                .clone()
        };
        assert_eq!(
            pts_of("s1"),
            Some("/dev/ttys002".to_string()),
            "in a cmux surface -> pts flows through to the row"
        );
        assert_eq!(
            pts_of("s2"),
            None,
            "outside cmux -> never highlightable, pts withheld even though ps knew the tty"
        );
    }

    fn minimal_row(id: &str, status: Status) -> SessionRow {
        SessionRow {
            session_id: id.to_string(),
            name: id.to_string(),
            account: None,
            status,
            cwd: String::new(),
            context_tokens: None,
            ctx_pct: None,
            secs_since_append: None,
            pane: None,
            pts: None,
        }
    }

    #[test]
    fn sort_rows_interleaves_codex_and_claude_rows_by_status_bucket() {
        // A concatenated Claude+Codex sweep (spec 008: rows carry no origin marker — the sort
        // must bucket purely on `status`, regardless of which sensor produced the row).
        let mut rows = vec![
            minimal_row("codex-idle", Status::Idle),
            minimal_row("claude-ask", Status::NeedsAnswer),
            minimal_row("codex-working", Status::Working),
            minimal_row("claude-stalled", Status::Stalled),
        ];
        sort_rows(&mut rows);
        let ids: Vec<&str> = rows.iter().map(|r| r.session_id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "claude-ask",
                "claude-stalled",
                "codex-working",
                "codex-idle"
            ],
            "attention buckets first, Claude/Codex interleaved by status alone"
        );
    }
}
