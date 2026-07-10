//! panes ctx: wezterm pane list — parse, classify Claude panes, build jump commands.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/panes.rs
//! Deps:    serde/serde_json; crate::runner (fetch only — parsing is pure)
//! Tested:  inline `#[cfg(test)]` against tests/fixtures/wezterm-list.json (captured live 2026-07-10)
//!
//! Key responsibilities:
//! - Discover ALL live wezterm instances (tasklist PIDs × gui-sock-<pid> files) — a `cli`
//!   call answers only from the instance owning the invoking pane's interop, so fleet running
//!   on the TUI monitor sees zero Claude panes unless each instance is targeted explicitly
//!   via a WSLENV-forwarded `WEZTERM_UNIX_SOCKET` (verified live 2026-07-10; flag `/w`).
//! - Parse `cli list --format json` tolerantly; classify titles (braille spinner = Working,
//!   `✳` = Idle, else not a Claude pane); merge instances; per-instance tab-bar numbering.
//! - Build `list` / `activate-tab` / `activate-pane` argv+env (pure).
//!
//! Design constraints:
//! - Glyph convention is undocumented (dossier assumption A2): classification must stay a pure
//!   table-tested function so a format change is a one-function fix.
//! - Stale gui-sock files HANG on connect (verified) — only tasklist-live PIDs are queried,
//!   every call stays timeout-bounded.
//! - Read-only over the fleet: the only mutating verbs are activate-tab/-pane (focus).

use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::runner::{CommandSpec, Runner};

/// Status of a Claude pane, read from its title glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneStatus {
    /// Title starts with a braille spinner frame (U+2800–U+28FF) — Claude is working.
    Working,
    /// Title starts with `✳` — Claude is idle (waiting for the user).
    Idle,
}

/// One Claude pane row on the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRow {
    /// Windows-form socket path of the owning wezterm instance (empty = invoker's own) —
    /// pane/tab ids are only unique WITHIN an instance, and jumps must target the right one.
    pub socket: String,
    /// wezterm pane id — identity and jump target.
    pub pane_id: u64,
    /// wezterm tab id — display grouping.
    pub tab_id: u64,
    /// 1-based position of this pane's tab within its window (the tab-bar number the maintainer sees;
    /// derived from list order, counting ALL tabs incl. non-Claude ones).
    pub tab_index: u64,
    /// Glyph-derived status.
    pub status: PaneStatus,
    /// Title with the glyph prefix stripped — the session's semantic name.
    pub name: String,
    /// Shortened cwd for display.
    pub cwd: String,
    /// Whether wezterm reports this pane as the active one.
    pub is_active: bool,
}

/// Raw wezterm pane entry — only the fields we read; everything else is skipped.
#[derive(Debug, Deserialize)]
struct RawPane {
    pane_id: u64,
    tab_id: u64,
    #[serde(default)]
    window_id: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    is_active: bool,
}

/// argv for `wezterm.exe cli list --format json`.
pub fn list_args() -> Vec<String> {
    ["cli", "list", "--format", "json"]
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// argv for `wezterm.exe cli activate-pane --pane-id <id>`.
pub fn activate_pane_args(pane_id: u64) -> Vec<String> {
    vec![
        "cli".to_string(),
        "activate-pane".to_string(),
        "--pane-id".to_string(),
        pane_id.to_string(),
    ]
}

/// argv for `wezterm.exe cli activate-tab --tab-id <id>` — activate-pane alone focuses the
/// pane within its tab but does NOT bring the tab forward; a jump runs both.
pub fn activate_tab_args(tab_id: u64) -> Vec<String> {
    vec![
        "cli".to_string(),
        "activate-tab".to_string(),
        "--tab-id".to_string(),
        tab_id.to_string(),
    ]
}

/// The wezterm binary as reachable from WSL2.
pub const WEZTERM: &str = "wezterm.exe";
/// Where the interop binary actually lives on this box — the fallback when fleet is launched
/// with a minimal PATH (keybinding/launcher shells often lack /mnt/c/...).
const WEZTERM_ABSOLUTE: &str = "/mnt/c/Program Files/WezTerm/wezterm.exe";

/// Resolve the wezterm program: plain name when PATH can find it (normal shells), the absolute
/// install path when it can't but the file exists, else the plain name (spawn error stays
/// visible in the footer). Pure over its inputs for testability.
fn resolve_wezterm(path_var: Option<&std::ffi::OsStr>, absolute: &std::path::Path) -> String {
    let on_path =
        path_var.is_some_and(|p| std::env::split_paths(p).any(|dir| dir.join(WEZTERM).is_file()));
    if on_path {
        WEZTERM.to_string()
    } else if absolute.is_file() {
        absolute.to_string_lossy().into_owned()
    } else {
        WEZTERM.to_string()
    }
}

/// The resolved program, computed once per process.
fn wezterm_program() -> String {
    static PROGRAM: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PROGRAM
        .get_or_init(|| {
            let path_var = std::env::var_os("PATH");
            resolve_wezterm(path_var.as_deref(), std::path::Path::new(WEZTERM_ABSOLUTE))
        })
        .clone()
}

/// Where wezterm instance sockets live, WSL- and Windows-form. This box's layout (single-user
/// tool); doctor prints what was found there.
const SOCK_DIR_WSL: &str = "/mnt/c/Users/user/.local/share/wezterm";
const SOCK_DIR_WIN: &str = "C:\\Users\\user\\.local\\share\\wezterm";

/// argv for `tasklist.exe` filtered to wezterm-gui processes, CSV form.
pub fn tasklist_args() -> Vec<String> {
    ["/FI", "IMAGENAME eq wezterm-gui.exe", "/FO", "CSV"]
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// Build the bounded `tasklist.exe` command.
pub fn tasklist_spec() -> CommandSpec {
    CommandSpec {
        program: "tasklist.exe".to_string(),
        args: tasklist_args(),
        env: Vec::new(),
        timeout: Duration::from_secs(5),
    }
}

/// Parse tasklist CSV → wezterm-gui PIDs. Tolerant: malformed lines skipped.
pub fn parse_tasklist_pids(bytes: &[u8]) -> Vec<u32> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split("\",\"");
            let image = fields.next()?.trim_start_matches('"');
            if !image.eq_ignore_ascii_case("wezterm-gui.exe") {
                return None;
            }
            fields.next()?.parse().ok()
        })
        .collect()
}

/// The env pair that targets one wezterm instance from WSL: the socket var itself plus a
/// per-process WSLENV telling interop to forward it (flag `/w` = WSL→Win32 direction).
fn socket_env(socket_win: &str) -> Vec<(String, String)> {
    vec![
        ("WEZTERM_UNIX_SOCKET".to_string(), socket_win.to_string()),
        ("WSLENV".to_string(), "WEZTERM_UNIX_SOCKET/w".to_string()),
    ]
}

/// Discover live instances: tasklist PIDs whose `gui-sock-<pid>` file exists.
/// Dead PIDs' stale socket files HANG on connect — this filter is load-bearing.
pub async fn discover_sockets(runner: &dyn Runner) -> AppResult<Vec<String>> {
    let bytes = runner.run(&tasklist_spec()).await?;
    let pids = parse_tasklist_pids(&bytes);
    Ok(pids
        .into_iter()
        .filter(|pid| {
            std::path::Path::new(SOCK_DIR_WSL)
                .join(format!("gui-sock-{pid}"))
                .is_file()
        })
        .map(|pid| format!("{SOCK_DIR_WIN}\\gui-sock-{pid}"))
        .collect())
}

/// Query every live instance and merge their Claude panes. Degrades per instance: one failing
/// instance is skipped; only total failure (or discovery failure) is an error.
pub async fn list_all_panes(runner: &dyn Runner) -> AppResult<Vec<PaneRow>> {
    let sockets = discover_sockets(runner).await?;
    if sockets.is_empty() {
        // No instance discovered (tasklist empty?) — fall back to the invoker's own instance.
        return list_panes(runner, "").await;
    }
    let queries = sockets.iter().map(|s| list_panes(runner, s));
    let results = futures::future::join_all(queries).await;
    let mut merged = Vec::new();
    let mut last_err = None;
    for result in results {
        match result {
            Ok(mut rows) => merged.append(&mut rows),
            Err(e) => last_err = Some(e),
        }
    }
    if merged.is_empty() {
        if let Some(e) = last_err {
            return Err(e);
        }
    }
    Ok(merged)
}

/// Last-good pane list: a wezterm lane error must not blank the board's TAB/PANE columns —
/// stale matches (with the error in the footer) beat no matches.
#[derive(Debug, Default)]
pub struct PaneCache {
    last: Vec<PaneRow>,
}

impl PaneCache {
    /// Fold a lane result: success replaces the cache; failure returns the last good list
    /// plus the error string for the footer.
    pub fn fold(&mut self, result: AppResult<Vec<PaneRow>>) -> (Vec<PaneRow>, Option<String>) {
        match result {
            Ok(rows) => {
                self.last.clone_from(&rows);
                (rows, None)
            }
            Err(e) => (self.last.clone(), Some(e.to_string())),
        }
    }
}

/// Build the bounded `cli list` command against one instance ("" = invoker's own).
pub fn list_spec(socket_win: &str) -> CommandSpec {
    CommandSpec {
        program: wezterm_program(),
        args: list_args(),
        env: if socket_win.is_empty() {
            Vec::new()
        } else {
            socket_env(socket_win)
        },
        timeout: Duration::from_secs(5),
    }
}

/// Build the bounded `activate-pane` command against the pane's instance.
pub fn activate_pane_spec(socket_win: &str, pane_id: u64) -> CommandSpec {
    CommandSpec {
        program: wezterm_program(),
        args: activate_pane_args(pane_id),
        env: if socket_win.is_empty() {
            Vec::new()
        } else {
            socket_env(socket_win)
        },
        timeout: Duration::from_secs(5),
    }
}

/// Build the bounded `activate-tab` command against the pane's instance.
pub fn activate_tab_spec(socket_win: &str, tab_id: u64) -> CommandSpec {
    CommandSpec {
        program: wezterm_program(),
        args: activate_tab_args(tab_id),
        env: if socket_win.is_empty() {
            Vec::new()
        } else {
            socket_env(socket_win)
        },
        timeout: Duration::from_secs(5),
    }
}

/// Run `cli list` against one instance and return its Claude pane rows, sorted by `pane_id`.
pub async fn list_panes(runner: &dyn Runner, socket_win: &str) -> AppResult<Vec<PaneRow>> {
    let bytes = runner.run(&list_spec(socket_win)).await?;
    parse_pane_list(&bytes, socket_win)
}

/// Parse `cli list --format json` bytes into Claude pane rows, sorted by `pane_id`.
/// Non-Claude panes (no recognized glyph) are excluded; rows are stamped with their instance.
pub fn parse_pane_list(bytes: &[u8], socket_win: &str) -> AppResult<Vec<PaneRow>> {
    let raw: Vec<RawPane> =
        serde_json::from_slice(bytes).map_err(|e| AppError::Parse(format!("wezterm list: {e}")))?;
    // Tab-bar numbering: wezterm lists panes in window/tab order, so a tab's 1-based position
    // within its window = order of first appearance. Counted over ALL panes (non-Claude tabs
    // occupy tab-bar slots too) BEFORE the pane_id sort below destroys that order.
    let mut tab_positions: std::collections::HashMap<(u64, u64), u64> =
        std::collections::HashMap::new();
    let mut per_window: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    for p in &raw {
        tab_positions
            .entry((p.window_id, p.tab_id))
            .or_insert_with(|| {
                let counter = per_window.entry(p.window_id).or_insert(0);
                *counter += 1;
                *counter
            });
    }
    let mut rows: Vec<PaneRow> = raw
        .into_iter()
        .filter_map(|p| {
            let (status, name) = classify_title(&p.title)?;
            Some(PaneRow {
                socket: socket_win.to_string(),
                pane_id: p.pane_id,
                tab_id: p.tab_id,
                tab_index: tab_positions
                    .get(&(p.window_id, p.tab_id))
                    .copied()
                    .unwrap_or(0),
                status,
                name,
                cwd: short_cwd(&p.cwd),
                is_active: p.is_active,
            })
        })
        .collect();
    rows.sort_by_key(|r| r.pane_id);
    Ok(rows)
}

/// Classify a pane title by its leading glyph; `None` = not a Claude pane.
/// Returns the status and the title with glyph + following whitespace stripped.
fn classify_title(title: &str) -> Option<(PaneStatus, String)> {
    let mut chars = title.chars();
    let first = chars.next()?;
    let status = match first {
        '\u{2800}'..='\u{28FF}' => PaneStatus::Working,
        '✳' => PaneStatus::Idle,
        _ => return None,
    };
    Some((status, chars.as_str().trim_start().to_string()))
}

/// Shorten a wezterm `file://` cwd URL for display.
/// `file://wsl.localhost/<distro>/a/b` → `/a/b`; `file:///C:/x/y` → `C:/x/y`; else verbatim.
fn short_cwd(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches('/');
    if let Some(rest) = trimmed.strip_prefix("file://wsl.localhost/") {
        // Drop the distro segment, keep the absolute WSL path.
        return match rest.split_once('/') {
            Some((_distro, path)) => format!("/{path}"),
            None => "/".to_string(),
        };
    }
    if let Some(rest) = trimmed.strip_prefix("file:///") {
        return rest.to_string();
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::CannedRunner;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/wezterm-list.json");

    #[test]
    fn fixture_parses_to_claude_rows_only_sorted_by_pane_id() {
        let rows = parse_pane_list(FIXTURE, "").expect("fixture parses");
        assert!(!rows.is_empty(), "fixture has Claude panes");
        assert!(rows.windows(2).all(|w| w[0].pane_id < w[1].pane_id));
        // The fixture contains wslhost.exe and empty-title panes — none may survive.
        assert!(rows.iter().all(|r| !r.name.contains("wslhost")));
    }

    #[test]
    fn fixture_row_fields_are_extracted() {
        let rows = parse_pane_list(FIXTURE, "").expect("fixture parses");
        let fleet = rows
            .iter()
            .find(|r| r.name.contains("FleetOps"))
            .expect("this session's pane is in the fixture");
        assert_eq!(fleet.status, PaneStatus::Working);
        assert_eq!(fleet.cwd, "/tui/fleetops");
        // Fixture order: tab 1 first, then tab 3 (this pane) — 2nd slot on the tab bar.
        assert_eq!(fleet.tab_index, 2);
    }

    #[test]
    fn classify_title_table() {
        let cases: &[(&str, Option<(PaneStatus, &str)>)] = &[
            ("⠂ Fix the bug", Some((PaneStatus::Working, "Fix the bug"))),
            ("⠐ Resume", Some((PaneStatus::Working, "Resume"))),
            ("⣿dense", Some((PaneStatus::Working, "dense"))),
            ("✳ Review skills", Some((PaneStatus::Idle, "Review skills"))),
            ("✳", Some((PaneStatus::Idle, ""))),
            ("wslhost.exe", None),
            ("", None),
            ("→ arrow title", None),
            ("plain shell", None),
        ];
        for (title, want) in cases {
            let got = classify_title(title);
            let want = want.map(|(s, n)| (s, n.to_string()));
            assert_eq!(got, want, "title {title:?}");
        }
    }

    #[test]
    fn short_cwd_table() {
        let cases = [
            ("file://wsl.localhost/Ubuntu/tui/fleetops/", "/tui/fleetops"),
            ("file://wsl.localhost/Ubuntu/", "/"),
            ("file:///C:/Users/user/", "C:/Users/user"),
            ("", ""),
            ("weird", "weird"),
        ];
        for (input, want) in cases {
            assert_eq!(short_cwd(input), want, "cwd {input:?}");
        }
    }

    #[test]
    fn unknown_fields_and_missing_optionals_are_tolerated() {
        let json = r#"[{"pane_id": 7, "tab_id": 1, "title": "⠢ x", "novel_field": {"a": 1}}]"#;
        let rows = parse_pane_list(json.as_bytes(), "").expect("tolerant parse");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pane_id, 7);
        assert_eq!(rows[0].cwd, "");
        assert!(!rows[0].is_active);
    }

    #[test]
    fn garbage_input_is_a_parse_error() {
        assert!(matches!(
            parse_pane_list(b"not json", ""),
            Err(AppError::Parse(_))
        ));
    }

    #[test]
    fn argv_builders() {
        assert_eq!(list_args(), ["cli", "list", "--format", "json"]);
        assert_eq!(
            activate_pane_args(42),
            ["cli", "activate-pane", "--pane-id", "42"]
        );
        assert_eq!(
            activate_tab_args(7),
            ["cli", "activate-tab", "--tab-id", "7"]
        );
    }

    #[test]
    fn resolve_wezterm_prefers_path_then_absolute_fallback() {
        let tmp = std::env::temp_dir().join(format!("fleet-wez-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let exe = tmp.join("wezterm.exe");

        // Not on PATH, fallback file exists → absolute fallback wins.
        std::fs::write(&exe, b"").unwrap();
        let path_var = std::ffi::OsString::from("/nonexistent-dir");
        assert_eq!(
            resolve_wezterm(Some(&path_var), &exe),
            exe.to_string_lossy().as_ref()
        );

        // On PATH → plain program name (PATH resolution at spawn).
        let path_var = std::ffi::OsString::from(format!("/nonexistent-dir:{}", tmp.display()));
        assert_eq!(resolve_wezterm(Some(&path_var), &exe), WEZTERM);

        // Neither → plain name (spawn error stays visible in the footer).
        std::fs::remove_file(&exe).unwrap();
        let path_var = std::ffi::OsString::from("/nonexistent-dir");
        assert_eq!(resolve_wezterm(Some(&path_var), &exe), WEZTERM);
        assert_eq!(resolve_wezterm(None, &exe), WEZTERM);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn pane_cache_keeps_last_good_list_on_lane_error() {
        let mut cache = PaneCache::default();
        let rows = vec![PaneRow {
            socket: String::new(),
            pane_id: 1,
            tab_id: 1,
            tab_index: 1,
            status: PaneStatus::Working,
            name: "x".to_string(),
            cwd: "/x".to_string(),
            is_active: false,
        }];

        // Success populates the cache and reports no error.
        let (got, err) = cache.fold(Ok(rows.clone()));
        assert_eq!(got, rows);
        assert_eq!(err, None);

        // Failure returns the LAST GOOD list (stale matches beat no matches) + the error.
        let (got, err) = cache.fold(Err(AppError::Timeout {
            program: WEZTERM.to_string(),
            seconds: 5,
        }));
        assert_eq!(got, rows, "stale pane list survives a lane error");
        assert!(err.is_some_and(|e| e.contains("timed out")));

        // Next success replaces it again.
        let (got, err) = cache.fold(Ok(Vec::new()));
        assert!(got.is_empty());
        assert_eq!(err, None);
    }

    #[tokio::test]
    async fn list_panes_runs_the_list_spec() {
        let runner = CannedRunner::new(FIXTURE.to_vec());
        let rows = list_panes(&runner, "").await.expect("canned list parses");
        assert!(!rows.is_empty());
        let spec = runner.last_spec().expect("spec recorded");
        // Program resolves via PATH or the absolute fallback depending on the test env.
        assert!(spec.program.ends_with(WEZTERM), "got {}", spec.program);
        assert_eq!(spec.args, list_args());
    }

    #[test]
    fn tasklist_csv_parses_to_pids() {
        let csv = b"\"Image Name\",\"PID\",\"Session Name\",\"Session#\",\"Mem Usage\"\r\n\
\"wezterm-gui.exe\",\"18840\",\"Console\",\"1\",\"139,280 K\"\r\n\
\"wezterm-gui.exe\",\"3428\",\"Console\",\"1\",\"218,680 K\"\r\n\
garbage line\r\n";
        assert_eq!(parse_tasklist_pids(csv), vec![18_840, 3_428]);
        assert!(parse_tasklist_pids(b"INFO: No tasks are running.\r\n").is_empty());
    }

    #[tokio::test]
    async fn list_panes_stamps_rows_with_their_instance_and_targets_it() {
        let runner = CannedRunner::new_seq(vec![Ok(FIXTURE.to_vec()), Ok(FIXTURE.to_vec())]);
        let a = list_panes(&runner, "C:\\sock-a").await.expect("instance a");
        let b = list_panes(&runner, "C:\\sock-b").await.expect("instance b");
        assert!(a.iter().all(|p| p.socket == "C:\\sock-a"));
        assert!(b.iter().all(|p| p.socket == "C:\\sock-b"));

        let specs = runner.all_specs();
        assert_eq!(specs.len(), 2);
        // Each call carries the WSLENV-forwarded socket env targeting ITS instance.
        assert_eq!(
            specs[0].env,
            vec![
                ("WEZTERM_UNIX_SOCKET".to_string(), "C:\\sock-a".to_string()),
                ("WSLENV".to_string(), "WEZTERM_UNIX_SOCKET/w".to_string()),
            ]
        );
        assert_eq!(specs[1].env[0].1, "C:\\sock-b");
    }

    #[test]
    fn jump_specs_carry_the_socket_env() {
        let tab = activate_tab_spec("C:\\sock-a", 7);
        assert_eq!(tab.args, ["cli", "activate-tab", "--tab-id", "7"]);
        assert_eq!(tab.env[0].1, "C:\\sock-a");
        let pane = activate_pane_spec("", 9);
        assert!(pane.env.is_empty(), "own instance needs no socket env");
    }
}
