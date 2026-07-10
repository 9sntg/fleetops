//! Pure render: the session board + footer, a function of `&App` only.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/tui/view.rs
//! Deps:    ratatui; board, fold, telemetry (formatting helpers)
//! Tested:  inline `#[cfg(test)]` TestBackend render assertions
//!
//! Key responsibilities:
//! - Board table: status | tab (tab-bar position) | session | account | ctx% | tokens | age |
//!   dir (last cwd folder) | pane.
//! - Stable per-account color (hash into a 6-color palette); status color from one pure map.
//! - Footer: session count, needs-answer count, refresh age, key hints, last error.
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
use crate::telemetry::{ctx_used_pct, format_tokens};

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

/// Stable account color: same name → same color, 6-slot palette (6 accounts on this box).
/// djb2 with seed 18 + a splitmix64 finalizer — the seed is chosen so the six current
/// accounts (echo-acct/gmail/acme/post/golf-acct/projectz) land on six DISTINCT slots (spec 004);
/// a future account still gets a stable color, possibly colliding (acceptable for a visual aid).
fn account_color(account: &str) -> Color {
    const PALETTE: [Color; 6] = [
        Color::Cyan,
        Color::LightMagenta,
        Color::LightBlue,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightRed,
    ];
    let mut x: u64 = account
        .bytes()
        .map(u64::from)
        .fold(18, |h, b| h.wrapping_mul(33) ^ b);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    PALETTE[usize::try_from(x % 6).unwrap_or(0)]
}

/// TAB cell: the 1-based tab-bar position (what the tab bar shows), `≈?`/`—` when unmatched.
fn tab_cell(row: &SessionRow) -> String {
    match (&row.pane, row.pane_ambiguous) {
        (Some(p), _) => p.tab_index.to_string(),
        (None, true) => "≈?".to_string(),
        (None, false) => "—".to_string(),
    }
}

fn pane_cell(row: &SessionRow) -> String {
    match (&row.pane, row.pane_ambiguous) {
        (Some(p), _) => p.pane_id.to_string(),
        (None, true) => "≈?".to_string(),
        (None, false) => "—".to_string(),
    }
}

/// Context gauge width in cells.
const CTX_BAR_WIDTH: u64 = 10;

/// Context gauge: filled blocks over the window, no percentage (visual per the maintainer's ask).
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
        "STATUS", "TAB", "SESSION", "ACCT", "CTX", "TOK", "AGE", "DIR", "PANE",
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
        let (ctx_cell, tok) = r.context_tokens.map_or_else(
            || (Cell::from("—"), "—".to_string()),
            |t| {
                let pct = ctx_used_pct(t);
                (
                    Cell::from(ctx_bar(pct)).style(ctx_style(pct)),
                    format_tokens(t),
                )
            },
        );
        let age_cell = r.secs_since_append.map_or_else(
            || Cell::from("—"),
            |secs| Cell::from(format_age(secs)).style(age_style(secs)),
        );
        let row = Row::new([
            Cell::from(label).style(style),
            Cell::from(tab_cell(r)),
            Cell::from(r.name.clone()),
            Cell::from(account).style(account_style),
            ctx_cell,
            Cell::from(tok),
            age_cell,
            Cell::from(dir_name(&r.cwd).to_string()),
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
            Constraint::Length(10),
            Constraint::Length(3),
            Constraint::Min(24),
            Constraint::Length(8), // longest real account name ("echo-acct"/"projectz")
            Constraint::Length(10),
            Constraint::Length(5),
            Constraint::Length(4),
            Constraint::Max(16),
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
    let stats = format!(
        "{dir_warn}{parse_warn}{} live · {} need answer · {} stale files · refreshed {}s ago",
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
    use super::*;
    use crate::discovery::ScanStats;
    use crate::tui::model::{Msg, Snapshot};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn row(id: &str, status: Status, name: &str, tokens: Option<u64>) -> SessionRow {
        SessionRow {
            session_id: id.to_string(),
            name: name.to_string(),
            account: Some("golf-acct".to_string()),
            status,
            cwd: "/tui/fleetops".to_string(),
            context_tokens: tokens,
            secs_since_append: Some(75),
            pane: Some(crate::board::MatchedPane {
                socket: String::new(),
                tab_id: 3,
                pane_id: 47,
                tab_index: 2,
            }),
            pane_ambiguous: false,
        }
    }

    fn rendered(app: &App) -> String {
        let backend = TestBackend::new(120, 12);
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
        assert!(screen.contains("golf-acct"));
        assert!(
            screen.contains("██████░░░░"),
            "120k of 200k = 60% → 6 of 10 blocks, no percentage text"
        );
        assert!(!screen.contains('%'), "spec: visual gauge, no percentages");
        assert!(screen.contains("117k"));
        assert!(screen.contains("1m"), "75s age humanized");
        assert!(screen.contains("TAB"), "tab-bar position column");
        assert!(screen.contains("47"), "pane id column");
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
        r.account = Some("echo-acct".to_string());
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
        assert!(screen.contains("echo-acct"), "8-char account not truncated");
    }

    #[test]
    fn account_color_is_stable_and_distinct_for_known_accounts() {
        let a = account_color("golf-acct");
        assert_eq!(a, account_color("golf-acct"), "stable");
        // The six real accounts on this box (~/.claude-acct) — spec 004: all distinct.
        // (The hash seed is tuned to this set; re-tune if an account is renamed.)
        let accounts = ["echo-acct", "gmail", "acme", "post", "golf-acct", "projectz"];
        let distinct: std::collections::HashSet<_> = accounts
            .iter()
            .map(|a| format!("{:?}", account_color(a)))
            .collect();
        assert_eq!(distinct.len(), 6, "all six accounts distinct: {distinct:?}");
    }

    #[test]
    fn empty_board_renders_without_panicking() {
        let app = App::default();
        let screen = rendered(&app);
        assert!(screen.contains("fleet — 0 sessions"));
    }
}
