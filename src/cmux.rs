//! The cmux lane: terminal topology + jump, via cmux's shipped AppleScript interface.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/cmux.rs
//! Deps:    runner (the subprocess seam), error
//! Tested:  inline `#[cfg(test)]` — fixture tests/fixtures/cmux-terminals.txt (captured live
//!          2026-07-16), argv builder tests, injection-safety test
//!
//! Key responsibilities:
//! - `list_spec` / `parse_list`: every cmux terminal (surface) with its window/tab position.
//! - `focus_spec`: bring a surface's window to the front and focus it — the whole jump, in one
//!   command (cmux's `focus` is documented as "Focus a terminal, bringing its window to the
//!   front"), replacing wezterm's ordered activate-tab → activate-pane pair.
//!
//! Design constraints:
//! - AppleScript, not the cmux control socket: the socket is gated by
//!   `automation.socketControlMode = cmuxOnly`, which would force `fleet` to run inside cmux and
//!   to handle `CMUX_SOCKET_CAPABILITY` — a credential. AppleScript needs no credential and works
//!   both inside and outside cmux. Verified live 2026-07-16 (spec 012).
//! - The surface id is passed as `osascript` **argv**, never interpolated into the script, so a
//!   hostile id is inert data rather than code. Verified by test.
//! - `character id 9` is bound OUTSIDE the `tell` block: cmux's dictionary defines a `tab`
//!   CLASS, which shadows AppleScript's `tab` (tab-character) constant inside `tell` and would
//!   silently emit the literal text "tab" as the delimiter.
//! - Never a shell string — explicit argv only (rules/rust/subprocess-safety.md).

use std::time::Duration;

use crate::error::AppResult;
use crate::runner::{CommandSpec, Runner};

/// One cmux terminal — a "surface" in cmux's Window → Workspace(tab) → Pane → Surface model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    /// The surface UUID. Identical to the `CMUX_SURFACE_ID` cmux exports into the terminal's
    /// environment, which is what makes pid → surface an EXACT join (spec 012).
    pub id: String,
    /// 1-based window position.
    pub window_index: u32,
    /// 1-based tab (workspace) position within its window.
    pub tab_index: u32,
    /// The workspace (tab) name — cmux's "workstream", glyph-stripped (spec 015).
    pub name: String,
    /// The terminal's working directory.
    pub cwd: String,
}

/// Emit one tab-separated row per terminal: `id \t window# \t tab# \t tabName \t cwd`.
/// `d` is bound outside the `tell` block — see the module header on the `tab` class shadowing.
const LIST_SCRIPT: &str = r#"set d to character id 9
set out to ""
tell application "cmux"
  set wi to 0
  repeat with w in windows
    set wi to wi + 1
    set ti to 0
    repeat with t in tabs of w
      set ti to ti + 1
      repeat with tm in terminals of t
        set out to out & (id of tm) & d & wi & d & ti & d & (name of t) & d & (working directory of tm) & linefeed
      end repeat
    end repeat
  end repeat
end tell
return out"#;

/// Focus the surface whose id is `argv[1]`. Returns `ok` / `notfound`; an unknown id is not an
/// error — the surface may have closed between the sweep and the keypress.
const FOCUS_SCRIPT: &str = r#"on run argv
set target to item 1 of argv
tell application "cmux"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with tm in terminals of t
        if (id of tm) is target then
          focus tm
          return "ok"
        end if
      end repeat
    end repeat
  end repeat
end tell
return "notfound"
end run"#;

/// The id of the surface the user is looking at right now, or empty. `try` makes "no window
/// open" a normal empty answer rather than an AppleScript error.
const FOCUSED_SCRIPT: &str = r#"tell application "cmux"
  try
    return id of (focused terminal of (selected tab of (front window)))
  on error
    return ""
  end try
end tell"#;

/// The focused-surface call.
pub fn focused_spec() -> CommandSpec {
    CommandSpec {
        program: "osascript".to_string(),
        args: vec!["-e".to_string(), FOCUSED_SCRIPT.to_string()],
        env: Vec::new(),
        timeout: Duration::from_secs(5),
    }
}

/// The topology call.
pub fn list_spec() -> CommandSpec {
    CommandSpec {
        program: "osascript".to_string(),
        args: vec!["-e".to_string(), LIST_SCRIPT.to_string()],
        env: Vec::new(),
        timeout: Duration::from_secs(5),
    }
}

/// The jump call. `surface_id` rides in as argv, so it is data and can never be script.
pub fn focus_spec(surface_id: &str) -> CommandSpec {
    CommandSpec {
        program: "osascript".to_string(),
        args: vec![
            "-e".to_string(),
            FOCUS_SCRIPT.to_string(),
            surface_id.to_string(),
        ],
        env: Vec::new(),
        timeout: Duration::from_secs(5),
    }
}

/// Parse the tab-separated topology. Malformed rows are skipped, never fatal — a drifted cmux
/// must cost the PANE column, not the board.
pub fn parse_list(bytes: &[u8]) -> Vec<Surface> {
    let text = String::from_utf8_lossy(bytes);
    text.lines().filter_map(parse_row).collect()
}

fn parse_row(line: &str) -> Option<Surface> {
    let mut fields = line.split('\t');
    let id = fields.next()?.trim();
    let window_index = fields.next()?.trim().parse().ok()?;
    let tab_index = fields.next()?.trim().parse().ok()?;
    let name = fields.next().unwrap_or_default();
    let cwd = fields.next().unwrap_or_default().trim();
    if id.is_empty() {
        return None;
    }
    Some(Surface {
        id: id.to_string(),
        window_index,
        tab_index,
        name: strip_status_glyph(name),
        cwd: cwd.to_string(),
    })
}

/// Drop a leading agent status glyph from a workspace name.
///
/// cmux auto-names a workspace after its agent's title until the user renames it, and that
/// auto-name carries the agent's status glyph (observed live: `✳ Check current global skills`,
/// `⠐ Review project rules and guidelines`). The glyph is status — which the STATUS column
/// already renders — so the STREAM column shows the name only (spec 015).
pub fn strip_status_glyph(name: &str) -> String {
    let mut chars = name.chars();
    let stripped = match chars.next() {
        // Braille spinner frames (working) and ✳ (idle) — the same convention wave 12 retired
        // `classify_title` for.
        Some('\u{2800}'..='\u{28FF}' | '✳') => chars.as_str(),
        _ => name,
    };
    stripped.trim().to_string()
}

/// Fetch the cmux topology. An error here means the jump lane is down (cmux not running, or
/// Automation permission denied) — the board still renders, minus the PANE column.
pub async fn list(runner: &dyn Runner) -> AppResult<Vec<Surface>> {
    let bytes = runner.run(&list_spec()).await?;
    Ok(parse_list(&bytes))
}

/// The surface the user is currently looking at, if any. Never an error for the caller: cmux not
/// running simply means "nothing focused" (mirrors the wezterm-era `focused_pane_id`).
pub async fn focused_surface_id(runner: &dyn Runner) -> Option<String> {
    let bytes = runner.run(&focused_spec()).await.ok()?;
    let id = String::from_utf8_lossy(&bytes).trim().to_string();
    (!id.is_empty()).then_some(id)
}

/// Focus a surface. Errors surface in the footer; `notfound` is a normal, non-error outcome.
pub async fn focus(runner: &dyn Runner, surface_id: &str) -> AppResult<()> {
    runner.run(&focus_spec(surface_id)).await.map(|_| ())
}

/// Last-good topology across sweeps: a transient `osascript` failure must not blank the PANE
/// column for one frame — stale beats blank, and the footer says the lane is degraded.
#[derive(Debug, Default)]
pub struct SurfaceCache {
    last_good: Vec<Surface>,
}

impl SurfaceCache {
    /// Fold a fetch result into `(surfaces, lane_error)`. A success replaces the cache; a failure
    /// keeps the last good list and reports the error.
    pub fn fold(&mut self, result: AppResult<Vec<Surface>>) -> (Vec<Surface>, Option<String>) {
        match result {
            Ok(surfaces) => {
                self.last_good.clone_from(&surfaces);
                (surfaces, None)
            }
            Err(e) => (self.last_good.clone(), Some(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/cmux-terminals.txt");

    #[test]
    fn fixture_parses_to_surfaces() {
        let surfaces = parse_list(FIXTURE);
        assert_eq!(surfaces.len(), 7, "live fixture: one window, seven tabs");
        assert_eq!(surfaces[0].id, "277E65DB-005E-4B3A-B4D3-A290839E4F3C");
        assert_eq!(surfaces[0].window_index, 1);
        assert_eq!(surfaces[0].tab_index, 1);
        assert_eq!(surfaces[0].name, "gtm-studio");
        assert_eq!(surfaces[0].cwd, "/Users/user/Desktop/groupon-gtm-studio");
        assert_eq!(surfaces[3].tab_index, 4);
    }

    #[test]
    fn fixture_carries_the_workstream_names() {
        let surfaces = parse_list(FIXTURE);
        let names: Vec<&str> = surfaces.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"gtm-studio"));
        assert!(names.contains(&"email-signal-capture"));
        // Two tabs share one cwd with different names — proof STREAM is not derivable from cwd,
        // and must come from the surface match (spec 015).
        let desktop = names
            .iter()
            .filter(|n| **n == "skills" || **n == "rules")
            .count();
        assert_eq!(desktop, 2);
        let same_cwd = surfaces
            .iter()
            .filter(|s| s.cwd == "/Users/user/Desktop")
            .count();
        assert_eq!(same_cwd, 2, "two workstreams, one cwd");
    }

    #[test]
    fn strip_status_glyph_table() {
        // cmux auto-names a workspace after its agent's title, glyph and all, until renamed.
        // Observed live 2026-07-16.
        assert_eq!(
            strip_status_glyph("✳ Check current global skills"),
            "Check current global skills"
        );
        assert_eq!(
            strip_status_glyph("⠐ Review project rules and guidelines"),
            "Review project rules and guidelines"
        );
        assert_eq!(strip_status_glyph("⣿ working"), "working");
        // A user-set name has no glyph and must survive intact.
        assert_eq!(strip_status_glyph("gtm-studio"), "gtm-studio");
        assert_eq!(strip_status_glyph("  padded  "), "padded");
        assert_eq!(strip_status_glyph(""), "");
        // A name that merely STARTS with a letter/emoji must not lose its first char.
        assert_eq!(strip_status_glyph("rules"), "rules");
        assert_eq!(strip_status_glyph("🚀 launch"), "🚀 launch");
    }

    #[test]
    fn surface_ids_are_unique_so_the_match_can_be_exact() {
        // The whole identity story: ids are UUIDs, unique across windows/tabs, so a pid's
        // CMUX_SURFACE_ID resolves to exactly one surface — no title/cwd tie-break needed.
        let surfaces = parse_list(FIXTURE);
        let mut ids: Vec<&str> = surfaces.iter().map(|s| s.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "surface ids must be unique");
    }

    #[test]
    fn malformed_rows_are_skipped_not_fatal() {
        let input = b"good-id\t1\t2\tstream-a\t/tmp\n\
                      \n\
                      missing-fields\n\
                      bad-index\tX\t2\tstream-b\t/tmp\n\
                      \t1\t1\tstream-c\t/tmp\n\
                      other-id\t2\t3\tstream-d\t/var\n";
        let surfaces = parse_list(input);
        assert_eq!(surfaces.len(), 2, "only the two well-formed rows survive");
        assert_eq!(surfaces[0].id, "good-id");
        assert_eq!(surfaces[1].id, "other-id");
        assert_eq!(surfaces[1].window_index, 2);
    }

    #[test]
    fn garbage_is_empty_not_a_panic() {
        assert!(parse_list(b"").is_empty());
        assert!(parse_list(b"total garbage").is_empty());
        assert!(parse_list(&[0xff, 0xfe]).is_empty());
    }

    #[tokio::test]
    async fn list_fetches_and_parses_with_no_process_spawn() {
        use crate::runner::CannedRunner;
        let runner = CannedRunner::new(FIXTURE.to_vec());
        let surfaces = list(&runner).await.expect("canned bytes parse");
        assert_eq!(surfaces.len(), 7);
        let spec = runner.last_spec().expect("one call");
        assert_eq!(spec.program, "osascript");
    }

    #[tokio::test]
    async fn focus_sends_exactly_one_call_carrying_the_id_as_argv() {
        use crate::runner::CannedRunner;
        let runner = CannedRunner::new(b"ok\n".to_vec());
        focus(&runner, "uuid-target").await.expect("focus ok");
        let specs = runner.all_specs();
        assert_eq!(
            specs.len(),
            1,
            "one call: cmux's `focus` raises the window AND focuses the surface, so the \
             wezterm-era activate-tab→activate-pane ordering hazard cannot recur"
        );
        assert_eq!(specs[0].args[2], "uuid-target");
    }

    #[tokio::test]
    async fn focused_surface_id_trims_and_treats_empty_as_none() {
        use crate::runner::CannedRunner;
        let runner = CannedRunner::new(b"02EC2459-2B77-4FA8-A51C-452E80CA19F8\n".to_vec());
        assert_eq!(
            focused_surface_id(&runner).await.as_deref(),
            Some("02EC2459-2B77-4FA8-A51C-452E80CA19F8"),
            "the trailing newline osascript emits must not become part of the id"
        );

        // cmux with no window open returns "" — that's "nothing focused", not an id.
        let empty = CannedRunner::new(b"\n".to_vec());
        assert_eq!(focused_surface_id(&empty).await, None);
    }

    #[tokio::test]
    async fn focused_surface_id_swallows_a_dead_cmux() {
        use crate::error::AppError;
        use crate::runner::CannedRunner;
        // cmux not running must read as "nothing focused", never fail the snapshot.
        let runner = CannedRunner::new_seq(vec![Err(AppError::Subprocess {
            program: "osascript".to_string(),
            message: "exit 1".to_string(),
        })]);
        assert_eq!(focused_surface_id(&runner).await, None);
    }

    #[test]
    fn cache_keeps_the_last_good_list_when_a_sweep_fails() {
        use crate::error::AppError;
        let mut cache = SurfaceCache::default();
        let good = parse_list(FIXTURE);
        let (rows, err) = cache.fold(Ok(good.clone()));
        assert_eq!(rows.len(), 7);
        assert!(err.is_none());

        let (rows, err) = cache.fold(Err(AppError::Timeout {
            program: "osascript".to_string(),
            seconds: 5,
        }));
        assert_eq!(rows, good, "stale beats blank — the PANE column survives");
        assert!(
            err.is_some(),
            "but the footer must say the lane is degraded"
        );
    }

    #[test]
    fn a_cwd_containing_spaces_survives() {
        // Tab-separated, not space-separated, precisely so this works.
        let surfaces = parse_list(b"id-1\t1\t1\tmy stream\t/Users/user/My Projects/a b\n");
        assert_eq!(surfaces[0].cwd, "/Users/user/My Projects/a b");
        assert_eq!(
            surfaces[0].name, "my stream",
            "a workstream name may contain spaces too"
        );
    }

    #[test]
    fn specs_are_explicit_argv_never_a_shell_string() {
        let list = list_spec();
        assert_eq!(list.program, "osascript");
        assert_eq!(list.args[0], "-e");
        assert!(list.args[1].contains("tell application \"cmux\""));
        assert!(
            list.args[1].starts_with("set d to character id 9"),
            "the delimiter must bind OUTSIDE the tell block: cmux's `tab` CLASS shadows \
             AppleScript's tab constant and would emit the literal text \"tab\""
        );
    }

    #[test]
    fn focus_passes_the_id_as_argv_so_it_can_never_be_script() {
        // Injection safety: a hostile id is argv[1], i.e. data. Verified live 2026-07-16 —
        // this payload returns "notfound" rather than executing.
        let hostile = r#"" & (do shell script "echo pwned") & ""#;
        let spec = focus_spec(hostile);
        assert_eq!(spec.args.len(), 3, "-e, script, id");
        assert_eq!(
            spec.args[2], hostile,
            "the id rides as its own argv element"
        );
        assert!(
            !spec.args[1].contains("pwned"),
            "the id must never be interpolated into the script body"
        );
        assert!(spec.args[1].contains("item 1 of argv"));
    }
}
