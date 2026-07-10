//! `fleet doctor` — read-only drift report: are the undocumented sources still parseable?
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/doctor.rs
//! Deps:    discovery, telemetry, board, panes, runner, paths
//! Tested:  inline `#[cfg(test)]` — report rendered pure from canned `DoctorFacts`
//!
//! Key responsibilities:
//! - Gather live samples (sessions scan, transcript presence, pane match, wezterm reachability).
//! - Render a human report; surface unknown status strings and parse failures (assumption A1/A2 drift).
//!
//! Design constraints:
//! - Strictly read-only: no file is ever written, nothing is repaired.
//! - Rendering is pure over `DoctorFacts` so the report is testable with canned facts.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::discovery::{self, NativeStatus, ScanStats};
use crate::runner::Runner;
use crate::telemetry::TailCache;
use crate::{board, panes, paths};

/// Everything the report renders — gathered once, rendered pure.
#[derive(Debug)]
pub struct DoctorFacts {
    /// Discovery tallies.
    pub scan: ScanStats,
    /// Unknown native status strings seen (drift signal).
    pub unknown_statuses: BTreeSet<String>,
    /// CC versions present in live session files.
    pub versions: BTreeSet<String>,
    /// Per live session: (name, transcript found, account attributed, pane matched).
    pub sessions: Vec<(String, bool, bool, bool)>,
    /// Sessions carrying exact WSLENV pane identity (spec 005).
    pub exact_panes: usize,
    /// Live wezterm instances discovered (tasklist × socket files).
    pub instances: usize,
    /// Ok(pane count) or the wezterm failure.
    pub wezterm: Result<usize, String>,
}

impl Default for DoctorFacts {
    fn default() -> Self {
        Self {
            scan: ScanStats::default(),
            unknown_statuses: BTreeSet::new(),
            versions: BTreeSet::new(),
            sessions: Vec::new(),
            exact_panes: 0,
            instances: 0,
            wezterm: Err("not checked".to_string()),
        }
    }
}

/// Render the report — pure. (`writeln!` into a String never fails; results discarded.)
pub fn render_report(facts: &DoctorFacts) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    out.push_str("fleet doctor — read-only drift report\n\n");
    let _ = writeln!(
        out,
        "session files: {} total · {} live · {} stale-dead · {} parse-failed",
        facts.scan.total_files, facts.scan.live, facts.scan.stale_dead, facts.scan.parse_failed
    );
    if facts.scan.dir_unreadable {
        out.push_str("  ⚠ sessions dir unreadable — scan failed, this is NOT an empty fleet\n");
    }
    if facts.scan.parse_failed > 0 {
        out.push_str("  ⚠ parse failures — sessions/<pid>.json format may have drifted (A1)\n");
    }
    if facts.unknown_statuses.is_empty() {
        out.push_str("native statuses: all known (busy/idle/shell/waiting)\n");
    } else {
        let _ = writeln!(
            out,
            "  ⚠ unknown native statuses: {:?} — fold shows these as Unknown",
            facts.unknown_statuses
        );
    }
    let versions = if facts.versions.is_empty() {
        "none".to_string()
    } else {
        facts
            .versions
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    };
    let _ = writeln!(out, "cc versions in live files: {versions}");
    let _ = writeln!(
        out,
        "pane identity: {} of {} sessions exact (WSLENV WEZTERM_PANE)",
        facts.exact_panes,
        facts.sessions.len()
    );
    match &facts.wezterm {
        Ok(count) => {
            let _ = writeln!(
                out,
                "wezterm: {} instances · {count} Claude panes",
                facts.instances
            );
        }
        Err(e) => {
            let _ = writeln!(
                out,
                "  ⚠ wezterm unreachable: {e} — jump lane degraded (A2)"
            );
        }
    }
    let _ = writeln!(out, "\nlive sessions ({}):", facts.sessions.len());
    for (name, transcript, account, pane) in &facts.sessions {
        let mark = |b: bool| if b { "✓" } else { "✗" };
        let _ = writeln!(
            out,
            "  {} transcript · {} account · {} pane — {name}",
            mark(*transcript),
            mark(*account),
            mark(*pane),
        );
    }
    out
}

/// Gather facts from the live system and render the report. Read-only.
/// Returns `(report, scan_ok)` — `scan_ok == false` means the scan itself failed (exit 1).
pub async fn run(runner: &dyn Runner) -> (String, bool) {
    let claude_dir = paths::claude_dir();
    let instances = panes::discover_sockets(runner).await.map_or(0, |s| s.len());
    let (wezterm, pane_list) = match panes::list_all_panes(runner).await {
        Ok(rows) => (Ok(rows.len()), rows),
        Err(e) => (Err(e.to_string()), Vec::new()),
    };

    let facts = tokio::task::spawn_blocking(move || {
        let (sessions, scan) = discovery::scan(&claude_dir.join("sessions"), Path::new("/proc"));
        let cache = Arc::new(Mutex::new(TailCache::default()));
        let mut guard = cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let projects = claude_dir.join("projects");
        let mut facts = DoctorFacts {
            scan,
            instances,
            wezterm,
            ..DoctorFacts::default()
        };
        for s in &sessions {
            if let NativeStatus::Other(unknown) = &s.file.status {
                facts.unknown_statuses.insert(unknown.clone());
            }
            if let Some(v) = &s.file.version {
                facts.versions.insert(v.clone());
            }
            let telemetry = guard.read(&projects, &s.file.cwd, &s.file.session_id);
            let ai_title = telemetry
                .facts
                .as_ref()
                .and_then(|f| f.ai_title.clone())
                .unwrap_or_default();
            if s.wezterm_pane.is_some() {
                facts.exact_panes += 1;
            }
            let (pane, _) = board::match_pane(
                s.wezterm_pane,
                &s.file.cwd,
                &[&ai_title, &s.file.name],
                &pane_list,
            );
            facts.sessions.push((
                s.file.name.clone(),
                telemetry
                    .facts
                    .as_ref()
                    .is_some_and(|f| f != &crate::telemetry::TailFacts::default())
                    || telemetry.secs_since_append.is_some(),
                s.account.is_some(),
                pane.is_some(),
            ));
        }
        facts
    })
    .await
    .unwrap_or_default();

    let scan_ok = !facts.scan.dir_unreadable;
    (render_report(&facts), scan_ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_covers_clean_state() {
        let facts = DoctorFacts {
            scan: ScanStats {
                total_files: 36,
                parse_failed: 0,
                stale_dead: 20,
                live: 16,
                ..ScanStats::default()
            },
            versions: ["2.1.206".to_string()].into(),
            sessions: vec![("fleetops".to_string(), true, true, true)],
            wezterm: Ok(23),
            instances: 2,
            ..DoctorFacts::default()
        };
        let report = render_report(&facts);
        assert!(report.contains("36 total · 16 live · 20 stale-dead · 0 parse-failed"));
        assert!(report.contains("all known"));
        assert!(report.contains("2.1.206"));
        assert!(report.contains("2 instances · 23 Claude panes"));
        assert!(report.contains("✓ transcript · ✓ account · ✓ pane — fleetops"));
        assert!(!report.contains('⚠'));
    }

    #[test]
    fn report_flags_every_drift_class() {
        let facts = DoctorFacts {
            scan: ScanStats {
                total_files: 3,
                parse_failed: 2,
                stale_dead: 0,
                live: 1,
                dir_unreadable: false,
            },
            unknown_statuses: ["pondering".to_string()].into(),
            sessions: vec![("mystery".to_string(), false, false, false)],
            wezterm: Err("wezterm.exe: timed out after 5s".to_string()),
            ..DoctorFacts::default()
        };
        let report = render_report(&facts);
        assert!(report.contains("parse failures"));
        assert!(report.contains("pondering"));
        assert!(report.contains("wezterm unreachable"));
        assert!(report.contains("✗ transcript · ✗ account · ✗ pane — mystery"));
    }

    #[test]
    fn unreadable_dir_is_flagged_not_an_empty_fleet() {
        let facts = DoctorFacts {
            scan: ScanStats {
                dir_unreadable: true,
                ..ScanStats::default()
            },
            ..DoctorFacts::default()
        };
        let report = render_report(&facts);
        assert!(report.contains("sessions dir unreadable"));
    }
}
