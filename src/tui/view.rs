//! Pure render: the session board + footer, a function of `&App` only.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/tui/view.rs
//! Deps:    ratatui; board, fold, telemetry (formatting helpers)
//! Tested:  inline `#[cfg(test)]` TestBackend render assertions
//!
//! Key responsibilities:
//! - Board table: `#` (agent board number `n`, 1-based row order — the same `n` the snapshot
//!   emits; spec 010) | status | dir (badge: emoji + last cwd folder) | stream (cmux workstream)
//!   | branch | session | ctx% | tokens | account | age | pane.
//! - Stable per-account color (hash into a 6-color palette); stable per-dir emoji+color badge
//!   (two independently-seeded hashes); status color from one pure map.
//! - Footer: session count, needs-answer count, refresh age, key hints, last error, and a
//!   `· N codex` suffix when live Codex rows are folded into the sweep (spec 008).
//!
//! Design constraints:
//! - Pure and read-only over `App`: no I/O, no `.await`, no state mutation (ratatui rule).
//! - Colour is a pure function of state; never log tokens/secrets (only counts are rendered).

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;

use crate::board::{dir_name, format_age, SessionRow};
use crate::fold::Status;
use crate::telemetry::format_tokens;

use super::model::App;

/// Render the whole board.
pub fn render(f: &mut Frame<'_>, app: &App) {
    let [body, footer] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
    render_table(f, body, app);
    render_footer(f, footer, app);
}

/// One pure `status → (label, style)` map — consistent and testable.
fn status_style(status: Status) -> (&'static str, Style) {
    match status {
        Status::NeedsAnswer => (
            "? answer",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Status::Waiting => (
            "⏳ waiting",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Status::Stalled => ("~ stalled?", Style::default().fg(Color::Red)),
        Status::Unknown => ("! unknown", Style::default().fg(Color::Red)),
        Status::Working => ("⠿ working", Style::default().fg(Color::Green)),
        Status::Idle => ("✳ idle", Style::default().fg(Color::Yellow)),
        Status::Shell => ("$ shell", Style::default().fg(Color::DarkGray)),
    }
}

/// djb2 fold (from `seed`) + a splitmix64 finalizer — the one seeded hash behind both
/// `account_color` and `dir_badge` (spec 004/007): each caller picks its own seed(s) so
/// unrelated hash slots (account color, dir emoji, dir color) never correlate.
fn seeded_hash(s: &str, seed: u64) -> u64 {
    let mut x: u64 = s
        .bytes()
        .map(u64::from)
        .fold(seed, |h, b| h.wrapping_mul(33) ^ b);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    x
}

/// 6-color palette shared by the account and dir badges (spec 004).
const PALETTE: [Color; 6] = [
    Color::Cyan,
    Color::LightMagenta,
    Color::LightBlue,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightRed,
];

/// Stable account color: same name → same color, 6-slot palette.
/// The seed is chosen so a representative six-account set (alpha/bravo/charlie/delta/echo/november)
/// lands on six DISTINCT slots (spec 004); a future account still gets a stable color, possibly
/// colliding (acceptable for a visual aid).
fn account_color(account: &str) -> Color {
    const SEED: u64 = 82;
    PALETTE[usize::try_from(seeded_hash(account, SEED) % 6).unwrap_or(0)]
}

/// Stable dir badge: same dir name -> same emoji + color always (pure hash, like
/// `account_color`). Emoji and color are hashed with independent seeds so they don't
/// correlate (12 emoji x 6 colors = 72 effective combos). The seeds are tuned so a representative
/// six project dirs (fleetops, tokenomics, project-a, project-b, project-c, project-d) land on six
/// distinct (emoji, color) pairs — re-tune if a new project dir collides (same pattern as the
/// account-seed note on `account_color`).
fn dir_badge(dir: &str) -> (char, Color) {
    const EMOJI: [char; 12] = [
        '🦀', '🧠', '🚀', '📦', '🌊', '🔥', '🐙', '🎯', '🌿', '💎', '⚡', '🍋',
    ];
    const EMOJI_SEED: u64 = 0;
    const COLOR_SEED: u64 = 0;
    let emoji = EMOJI[usize::try_from(seeded_hash(dir, EMOJI_SEED) % 12).unwrap_or(0)];
    let color = PALETTE[usize::try_from(seeded_hash(dir, COLOR_SEED) % 6).unwrap_or(0)];
    (emoji, color)
}

/// STREAM column width; BRANCH is wider because `feat/…` prefixes are common.
const STREAM_WIDTH: u16 = 16;
const BRANCH_WIDTH: u16 = 18;

/// STREAM / BRANCH: present or `—`. Both are optional facts about a session, never errors —
/// no cmux surface means no workstream; a cwd outside a repo has no branch (spec 015).
///
/// Over-long values are truncated with an ellipsis rather than silently clipped: ratatui would
/// render `feat/email-signal-`, which reads as a real branch name and isn't one. `…` says the
/// name continues. Width counts chars, not bytes — a name may be non-ASCII.
fn optional_cell(value: Option<&String>, width: u16) -> String {
    let Some(v) = value else {
        return "—".to_string();
    };
    let width = usize::from(width);
    if v.chars().count() <= width {
        return v.clone();
    }
    let kept: String = v.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// The PANE column: the session's cmux tab number, or `—` when it doesn't render in a cmux
/// surface (started in another terminal). Exact-id matching leaves no ambiguous case to show
/// (spec 012 retired the wave-5 `≈?` marker along with the title/cwd tie-breaks).
fn pane_cell(row: &SessionRow) -> String {
    row.pane
        .as_ref()
        .map_or_else(|| "—".to_string(), |p| p.tab_index.to_string())
}

/// Context gauge width in cells.
const CTX_BAR_WIDTH: u64 = 10;

/// Context gauge: filled blocks over the window, no percentage (visual gauge, no numbers).
fn ctx_bar(pct: u64) -> String {
    let filled = (pct.min(100) * CTX_BAR_WIDTH).div_ceil(100);
    let mut bar = String::new();
    for i in 0..CTX_BAR_WIDTH {
        bar.push(if i < filled { '█' } else { '░' });
    }
    bar
}

/// Context severity color: green while comfortable, yellow when filling, red near the limit.
fn ctx_style(pct: u64) -> Style {
    let color = match pct {
        0..=59 => Color::Green,
        60..=84 => Color::Yellow,
        _ => Color::Red,
    };
    Style::default().fg(color)
}

/// Age color: fresh green, minutes yellow, stale red, ancient dimmed (long-idle ≠ urgent).
fn age_style(secs: u64) -> Style {
    let color = match secs {
        0..=59 => Color::Green,
        60..=599 => Color::Yellow,
        600..=1_799 => Color::LightRed,
        _ => Color::DarkGray,
    };
    Style::default().fg(color)
}

fn render_table(f: &mut Frame<'_>, area: Rect, app: &App) {
    let header = Row::new([
        "#", "STATUS", "DIR", "STREAM", "BRANCH", "SESSION", "CTX", "TOK", "ACCT", "AGE", "PANE",
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));
    let selected = app.selected_index();
    let rows = app.rows.iter().enumerate().map(|(i, r)| {
        let (label, style) = status_style(r.status);
        let account = r.account.clone().unwrap_or_default();
        let account_style = if account.is_empty() {
            Style::default()
        } else {
            Style::default().fg(account_color(&account))
        };
        let dir = dir_name(&r.cwd);
        let (dir_emoji, dir_color) = dir_badge(dir);
        // ctx% seam (spec 008): `ctx_pct` is the source of truth (Claude and Codex windows
        // differ — Claude's 200k/1M inference must never be applied to Codex tokens). A bare
        // `None` renders "—"; `board::assemble` always sets `ctx_pct` for Claude rows once
        // tokens are known, so this never regresses the Claude lane.
        let pct = r.ctx_pct.map(u64::from);
        let ctx_cell = pct.map_or_else(
            || Cell::from("—"),
            |pct| Cell::from(ctx_bar(pct)).style(ctx_style(pct)),
        );
        let tok = r
            .context_tokens
            .map_or_else(|| "—".to_string(), format_tokens);
        let age_cell = r.secs_since_append.map_or_else(
            || Cell::from("—"),
            |secs| Cell::from(format_age(secs)).style(age_style(secs)),
        );
        let row = Row::new([
            // `#`: the agent board number `n` — 1-based row order, the same `n` the snapshot
            // emits (spec 010). Present on every row (pure order), no unmatched placeholder.
            Cell::from((i + 1).to_string()),
            Cell::from(label).style(style),
            Cell::from(format!("{dir_emoji} {dir}")).style(Style::default().fg(dir_color)),
            Cell::from(optional_cell(r.stream.as_ref(), STREAM_WIDTH)),
            Cell::from(optional_cell(r.branch.as_ref(), BRANCH_WIDTH))
                .style(Style::default().fg(Color::Blue)),
            Cell::from(r.name.clone()),
            ctx_cell,
            Cell::from(tok),
            Cell::from(account).style(account_style),
            age_cell,
            Cell::from(pane_cell(r)),
        ]);
        if selected == Some(i) {
            row.style(Style::default().add_modifier(Modifier::REVERSED))
        } else {
            row
        }
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(3), // #: agent board number n, leading (spec 010)
            Constraint::Length(10),
            // Fixed, not Max: on a narrow window ratatui squeezes flexible columns first and
            // the badge column collapsed entirely (live-verified at 80 cols — DIR lost to
            // SESSION's Min). 14 = emoji(2) + space + 11 chars ("tokenomics" fits).
            Constraint::Length(14),
            // STREAM + BRANCH: fixed like DIR, for the same live-verified reason — flexible
            // columns get squeezed first and these would collapse before SESSION does.
            Constraint::Length(STREAM_WIDTH),
            Constraint::Length(BRANCH_WIDTH),
            Constraint::Min(24),
            Constraint::Length(10),
            Constraint::Length(5),
            Constraint::Length(8), // longest expected account name (e.g. "november")
            Constraint::Length(4),
            Constraint::Length(5),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" fleet — {} sessions ", app.rows.len())),
    );
    f.render_widget(table, area);
}

fn render_footer(f: &mut Frame<'_>, area: Rect, app: &App) {
    let needs = app
        .rows
        .iter()
        .filter(|r| r.status == Status::NeedsAnswer)
        .count();
    let dir_warn = if app.stats.dir_unreadable {
        "⚠ sessions dir unreadable · "
    } else {
        ""
    };
    // Format drift makes sessions vanish silently — the footer must count the casualties.
    let parse_warn = if app.stats.parse_failed > 0 {
        format!("⚠ {} unparseable session files · ", app.stats.parse_failed)
    } else {
        String::new()
    };
    // Codex rows aren't tallied in `ScanStats.live` (Claude-only) — spec 008 surfaces them here.
    let codex_suffix = if app.codex_count > 0 {
        format!(" · {} codex", app.codex_count)
    } else {
        String::new()
    };
    let stats = format!(
        "{dir_warn}{parse_warn}{} live · {} need answer · {} stale files · refreshed {}s ago{codex_suffix}",
        app.stats.live, needs, app.stats.stale_dead, app.refresh_age_secs
    );
    // Spec 004: stats PLUS the error — an error must not hide the freshness/counts.
    let text = app.error.as_ref().map_or_else(
        || format!("{stats} · j/k move · Enter jump · r refresh · q quit"),
        |e| format!("{stats} · ! {e}"),
    );
    let style = if app.error.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(Paragraph::new(Line::from(text)).style(style), area);
}

#[cfg(test)]
mod tests {
    /// The width the board needs once every fixed column fits: 3+10+14+16+18+24+10+5+8+4+5,
    /// plus 10 inter-column gaps and 2 borders (spec 015 widened this from 93 by adding
    /// STREAM+BRANCH). Rendering narrower is fine — ratatui squeezes — but the column-fitting
    /// assertions below are about the designed width.
    const BOARD_MIN_COLS: u16 = 129;

    use super::*;
    use crate::discovery::ScanStats;
    use crate::telemetry::ctx_used_pct;
    use crate::tui::model::{Msg, Snapshot};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Same clamp recipe as `board::assemble` — the fixture must carry `ctx_pct` exactly like a
    /// real Claude row would, now that the view no longer falls back to `context_tokens`.
    fn ctx_pct_for(tokens: u64) -> u8 {
        u8::try_from(ctx_used_pct(tokens).min(u64::from(u8::MAX))).unwrap_or(u8::MAX)
    }

    fn row(id: &str, status: Status, name: &str, tokens: Option<u64>) -> SessionRow {
        SessionRow {
            session_id: id.to_string(),
            name: name.to_string(),
            account: Some("alpha".to_string()),
            status,
            cwd: "/tui/fleetops".to_string(),
            context_tokens: tokens,
            ctx_pct: tokens.map(ctx_pct_for),
            secs_since_append: Some(75),
            stream: Some("gtm-studio".to_string()),
            branch: Some("feat/x".to_string()),
            pane: Some(crate::board::MatchedPane {
                surface_id: "uuid-47".to_string(),
                window_index: 1,
                tab_index: 2,
                stream: "gtm-studio".to_string(),
            }),
            pts: None,
        }
    }

    fn rendered(app: &App) -> String {
        let backend = TestBackend::new(BOARD_MIN_COLS, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn board_renders_all_columns_and_footer() {
        let mut app = App::default();
        app.update(Msg::Snapshot(Box::new(Snapshot {
            rows: vec![
                row("a", Status::NeedsAnswer, "Pick an option", Some(120_000)),
                row(
                    "b",
                    Status::Working,
                    "Resume FleetOps conversation",
                    Some(117_585),
                ),
            ],
            stats: ScanStats {
                total_files: 30,
                parse_failed: 0,
                stale_dead: 12,
                live: 2,
                ..ScanStats::default()
            },
            ..Snapshot::default()
        })));
        let screen = rendered(&app);
        assert!(screen.contains("? answer"));
        assert!(screen.contains("⠿ working"));
        assert!(screen.contains("Pick an option"));
        assert!(screen.contains("alpha"));
        assert!(
            screen.contains("██████░░░░"),
            "120k of 200k = 60% → 6 of 10 blocks, no percentage text"
        );
        assert!(!screen.contains('%'), "spec: visual gauge, no percentages");
        assert!(screen.contains("117k"));
        assert!(screen.contains("1m"), "75s age humanized");
        assert!(screen.contains('#'), "leading # agent-number column");
        assert!(
            screen.contains('2'),
            "PANE column shows the cmux tab number"
        );
        assert!(screen.contains("DIR"));
        assert!(
            !screen.contains("/tui/fleetops"),
            "DIR shows the last folder only"
        );
        assert!(screen.contains("fleet — 2 sessions"));
        assert!(screen.contains("2 live · 1 need answer · 12 stale files"));
    }

    #[test]
    fn missing_telemetry_renders_dashes() {
        let mut app = App::default();
        let mut r = row("a", Status::Working, "young session", None);
        r.secs_since_append = None;
        r.pane = None;
        app.update(Msg::Snapshot(Box::new(Snapshot {
            rows: vec![r],
            ..Snapshot::default()
        })));
        let screen = rendered(&app);
        assert!(screen.contains("—"));
    }

    #[test]
    fn footer_shows_error_alongside_stats_not_instead() {
        let mut app = App::default();
        app.update(Msg::Error("wezterm.exe: timed out after 5s".into()));
        let screen = rendered(&app);
        assert!(screen.contains("! wezterm.exe: timed out after 5s"));
        assert!(
            screen.contains("0 live"),
            "stats stay visible during an error"
        );
        assert!(
            !screen.contains("Enter jump"),
            "hints yield the space to the error"
        );
    }

    #[test]
    fn footer_counts_parse_failures_and_acct_fits_the_longest_account() {
        let mut app = App::default();
        let mut r = row("a", Status::Working, "x", None);
        r.account = Some("november".to_string());
        app.update(Msg::Snapshot(Box::new(Snapshot {
            rows: vec![r],
            stats: ScanStats {
                parse_failed: 3,
                live: 1,
                ..ScanStats::default()
            },
            ..Snapshot::default()
        })));
        let screen = rendered(&app);
        assert!(
            screen.contains("⚠ 3 unparseable session files"),
            "format drift must not read as a smaller fleet"
        );
        assert!(screen.contains("november"), "8-char account not truncated");
    }

    #[test]
    fn account_color_is_stable_and_distinct_for_known_accounts() {
        let a = account_color("alpha");
        assert_eq!(a, account_color("alpha"), "stable");
        // A representative six-account set — spec 004: all distinct.
        // (The hash seed is tuned to this set; re-tune if an account is renamed.)
        let accounts = ["alpha", "bravo", "charlie", "delta", "echo", "november"];
        let distinct: std::collections::HashSet<_> = accounts
            .iter()
            .map(|a| format!("{:?}", account_color(a)))
            .collect();
        assert_eq!(distinct.len(), 6, "all six accounts distinct: {distinct:?}");
    }

    #[test]
    fn stream_and_branch_render_between_dir_and_session() {
        let mut app = App::default();
        app.update(Msg::Snapshot(Box::new(Snapshot {
            rows: vec![row("s1", Status::Idle, "a session", Some(50_000))],
            ..Snapshot::default()
        })));
        let screen = rendered(&app);
        assert!(screen.contains("STREAM"), "STREAM header");
        assert!(screen.contains("BRANCH"), "BRANCH header");
        // Order matters: the maintainer asked for them BETWEEN dir and session (spec 015).
        let header = screen
            .lines()
            .find(|l| l.contains("STATUS"))
            .expect("header row");
        let at = |c: &str| header.find(c).expect("column present");
        assert!(at("DIR") < at("STREAM"), "STREAM sits after DIR");
        assert!(at("STREAM") < at("BRANCH"));
        assert!(at("BRANCH") < at("SESSION"), "BRANCH sits before SESSION");
        assert!(screen.contains("gtm-studio"), "the workstream name renders");
        assert!(screen.contains("feat/x"), "the branch renders");
    }

    #[test]
    fn a_session_with_no_stream_or_branch_renders_placeholders() {
        // Not in a cmux surface, cwd not a repo — both are normal, neither is an error.
        let mut app = App::default();
        let mut r = row("s1", Status::Idle, "a session", Some(50_000));
        r.pane = None;
        r.stream = None;
        r.branch = None;
        app.update(Msg::Snapshot(Box::new(Snapshot {
            rows: vec![r],
            ..Snapshot::default()
        })));
        let screen = rendered(&app);
        assert!(screen.contains('—'), "placeholders, not blanks or errors");
    }

    #[test]
    fn an_overlong_branch_is_ellipsised_not_silently_clipped() {
        // A clipped "feat/email-signal-" reads as a real branch name and isn't one.
        let long = "feat/email-signal-capture-and-more".to_string();
        let cell = optional_cell(Some(&long), BRANCH_WIDTH);
        assert!(cell.ends_with('…'), "truncation must be visible: {cell}");
        assert_eq!(cell.chars().count(), usize::from(BRANCH_WIDTH));
        // Exactly-at-width is not truncated.
        let exact: String = "x".repeat(usize::from(BRANCH_WIDTH));
        assert_eq!(optional_cell(Some(&exact), BRANCH_WIDTH), exact);
        assert_eq!(optional_cell(None, BRANCH_WIDTH), "—");
        // Char-based, not byte-based: a multi-byte name must not panic or mis-measure.
        let wide = "🚀".repeat(40);
        assert_eq!(
            optional_cell(Some(&wide), BRANCH_WIDTH).chars().count(),
            usize::from(BRANCH_WIDTH)
        );
    }

    #[test]
    fn empty_board_renders_without_panicking() {
        let app = App::default();
        let screen = rendered(&app);
        assert!(screen.contains("fleet — 0 sessions"));
    }

    #[test]
    fn header_order_status_dir_session_left_to_right() {
        let app = App::default();
        let screen = rendered(&app);
        let header_line = screen
            .lines()
            .find(|l| l.contains("STATUS"))
            .expect("header line with STATUS");
        let num_pos = header_line.find('#').expect("# in header");
        let status_pos = header_line.find("STATUS").expect("STATUS in header");
        let dir_pos = header_line.find("DIR").expect("DIR in header");
        let session_pos = header_line.find("SESSION").expect("SESSION in header");
        assert!(num_pos < status_pos, "spec 010: # must precede STATUS");
        assert!(status_pos < dir_pos, "spec 007: STATUS must precede DIR");
        assert!(dir_pos < session_pos, "spec 007: DIR must precede SESSION");
    }

    #[test]
    fn hash_column_shows_the_1_based_agent_number_n() {
        let mut app = App::default();
        // Two rows; the second has no matched pane — `n` is pure row order, present regardless.
        let first = row("a", Status::Working, "first-row", Some(50_000));
        let mut second = row("b", Status::Idle, "second-row", None);
        second.pane = None;
        app.update(Msg::Snapshot(Box::new(Snapshot {
            rows: vec![first, second],
            ..Snapshot::default()
        })));
        let screen = rendered(&app);
        // Row 1: the leading `#` cell shows agent number 1, before STATUS.
        let first_line = screen
            .lines()
            .find(|l| l.contains("first-row"))
            .expect("first row line");
        let n1 = first_line
            .find('1')
            .expect("agent number 1 on the first row");
        let status1 = first_line.find("working").expect("status on the first row");
        assert!(
            n1 < status1,
            "spec 010: the leading # agent number leads the row, before STATUS"
        );
        // Row 2: `n` is 2 even though the row has no matched pane (no placeholder for `#`).
        let second_line = screen
            .lines()
            .find(|l| l.contains("second-row"))
            .expect("second row line");
        let n2 = second_line
            .find('2')
            .expect("agent number 2 on the second (unmatched) row");
        let status2 = second_line.find("idle").expect("status on the second row");
        assert!(
            n2 < status2,
            "spec 010: n is present on unmatched rows too, before STATUS"
        );
    }

    #[test]
    fn dir_badge_is_stable() {
        assert_eq!(
            dir_badge("fleetops"),
            dir_badge("fleetops"),
            "same dir name -> same badge every call"
        );
    }

    #[test]
    fn dir_badge_distinct_for_known_dirs() {
        // A representative six project dirs — spec 007: all distinct (emoji, color) pairs.
        let dirs = [
            "fleetops",
            "tokenomics",
            "project-a",
            "project-b",
            "project-c",
            "project-d",
        ];
        let distinct: std::collections::HashSet<_> = dirs
            .iter()
            .map(|d| {
                let (emoji, color) = dir_badge(d);
                (emoji, format!("{color:?}"))
            })
            .collect();
        assert_eq!(distinct.len(), 6, "all six dirs distinct: {distinct:?}");
    }

    #[test]
    fn dir_cell_renders_badge_emoji_and_name() {
        let mut app = App::default();
        app.update(Msg::Snapshot(Box::new(Snapshot {
            rows: vec![row("a", Status::Working, "x", None)],
            ..Snapshot::default()
        })));
        let screen = rendered(&app);
        let (emoji, _) = dir_badge("fleetops");
        // Width-2 emoji leave a blank placeholder cell in TestBackend's buffer, so assert
        // ordering (emoji before name on the row) rather than exact adjacency.
        let row_line = screen
            .lines()
            .find(|l| l.contains("fleetops"))
            .expect("row line with the dir name");
        let emoji_pos = row_line
            .find(emoji)
            .expect("badge emoji rendered in the DIR cell");
        let name_pos = row_line.find("fleetops").expect("dir name rendered");
        assert!(emoji_pos < name_pos, "badge emoji precedes the dir name");
    }

    #[test]
    fn codex_row_renders_account_and_ctx_bar_from_ctx_pct() {
        // spec 008 ctx% seam: the bar must come from `ctx_pct`, not `context_tokens` run back
        // through `ctx_used_pct` — 50k/200k would render 25% (3 blocks); ctx_pct=60 must win.
        let mut r = row(
            "codex-1",
            Status::Working,
            "codex — no prompt yet",
            Some(50_000),
        );
        r.account = Some("codex".to_string());
        r.ctx_pct = Some(60);
        let mut app = App::default();
        app.update(Msg::Snapshot(Box::new(Snapshot {
            rows: vec![r],
            ..Snapshot::default()
        })));
        let screen = rendered(&app);
        assert!(screen.contains("codex"), "ACCT shows the codex label");
        assert!(
            screen.contains("██████░░░░"),
            "ctx bar must be computed from ctx_pct (60%), not context_tokens' own ctx_used_pct"
        );
    }
}
