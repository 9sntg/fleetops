//! `fleet doctor` — read-only drift report: are the undocumented sources still parseable?
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/doctor.rs
//! Deps:    discovery, telemetry, board, panes, runner, paths, procsrc
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

use crate::discovery::{self, NativeStatus, ScanStats};
use crate::runner::Runner;
use crate::telemetry::TailCache;
use crate::{board, panes, paths, procsrc};

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
    /// One instance answered, another failed — the pane list is partial.
    pub instance_error: Option<String>,
    /// The `ps` process table could not be fetched — liveness is unknowable, so every session
    /// reads as dead. An empty board here is a broken sensor, not an empty fleet.
    pub procs_error: Option<String>,
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
            instance_error: None,
            procs_error: None,
        }
    }
}

/// Render the report — pure. (`writeln!` into a String never fails; results discarded.)
pub fn render_report(facts: &DoctorFacts) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    out.push_str("fleet doctor — read-only drift report\n\n");
    if let Some(e) = &facts.procs_error {
        let _ = writeln!(
            out,
            "  ⚠ process table unavailable: {e} — every session reads as dead, this is NOT an empty fleet"
        );
    }
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
    if let Some(e) = &facts.instance_error {
        let _ = writeln!(
            out,
            "  ⚠ instance degraded: {e} — pane list is PARTIAL, counts above undercount"
        );
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
/// Returns `(report, scan_ok)` — `scan_ok == false` means the scan itself failed (exit 1):
/// either the sessions dir was unreadable or the `ps` process table was unavailable.
pub async fn run(runner: &dyn Runner) -> (String, bool) {
    let claude_dir = paths::claude_dir();
    let instances = panes::discover_sockets(runner).await.map_or(0, |s| s.len());
    let (wezterm, pane_list, instance_error) = match panes::list_all_panes(runner).await {
        Ok((rows, partial)) => (Ok(rows.len()), rows, partial),
        Err(e) => (Err(e.to_string()), Vec::new(), None),
    };
    // Liveness comes from `ps`; a failure here means every session reads as dead, which must
    // never be reported as a clean, empty fleet.
    let (procs, procs_error) = match procsrc::fetch(runner).await {
        Ok(table) => (table, None),
        Err(e) => (crate::procsrc::ProcTable::new(), Some(e.to_string())),
    };

    let facts = tokio::task::spawn_blocking(move || {
        let (sessions, mut scan) = discovery::scan(&claude_dir.join("sessions"), &procs);
        scan.procs_unavailable = procs_error.is_some();
        let mut cache = TailCache::default();
        let projects = claude_dir.join("projects");
        let mut facts = DoctorFacts {
            scan,
            instances,
            wezterm,
            instance_error,
            procs_error,
            ..DoctorFacts::default()
        };
        for s in &sessions {
            if let NativeStatus::Other(unknown) = &s.file.status {
                facts.unknown_statuses.insert(unknown.clone());
            }
            if let Some(v) = &s.file.version {
                facts.versions.insert(v.clone());
            }
            let telemetry = cache.read(&projects, &s.file.cwd, &s.file.session_id);
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
            // facts is Some iff the transcript existed and was readable (TailCache::read).
            facts.sessions.push((
                s.file.name.clone(),
                telemetry.facts.is_some(),
                s.account.is_some(),
                pane.is_some(),
            ));
        }
        facts
    })
    .await;
    // A crashed scan task must not render as a clean, empty fleet with exit 0.
    let Ok(facts) = facts else {
        return ("fleet doctor: scan task failed\n".to_string(), false);
    };

    // Both failures mean the numbers below are not a fleet reading: the dir couldn't be read, or
    // liveness couldn't be established. Either way, exit non-zero rather than look clean.
    let scan_ok = !facts.scan.dir_unreadable && !facts.scan.procs_unavailable;
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
            exact_panes: 1,
            wezterm: Ok(23),
            instances: 2,
            ..DoctorFacts::default()
        };
        let report = render_report(&facts);
        assert!(report.contains("36 total · 16 live · 20 stale-dead · 0 parse-failed"));
        assert!(report.contains("all known"));
        assert!(report.contains("2.1.206"));
        // Spec 005: the pane-identity adoption line.
        assert!(report.contains("pane identity: 1 of 1 sessions exact (WSLENV WEZTERM_PANE)"));
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
                ..ScanStats::default()
            },
            unknown_statuses: ["pondering".to_string()].into(),
            sessions: vec![("mystery".to_string(), false, false, false)],
            wezterm: Err("wezterm.exe: timed out after 5s".to_string()),
            instance_error: Some("gui-sock-3428: timed out after 5s".to_string()),
            ..DoctorFacts::default()
        };
        let report = render_report(&facts);
        assert!(report.contains("parse failures"));
        assert!(report.contains("pondering"));
        assert!(report.contains("wezterm unreachable"));
        assert!(
            report.contains("instance degraded"),
            "partial pane list is a drift class"
        );
        assert!(report.contains("✗ transcript · ✗ account · ✗ pane — mystery"));
    }

    #[test]
    fn unavailable_process_table_is_flagged_not_an_empty_fleet() {
        // Replaces wave-10's `missing_proc_prints_a_wsl2_platform_hint`. On Linux a missing
        // /proc and a dead PID shared one silent code path; the board read "nothing running".
        // The macOS source must say the sensor broke.
        let facts = DoctorFacts {
            procs_error: Some("ps: exit 1".to_string()),
            ..DoctorFacts::default()
        };
        let report = render_report(&facts);
        assert!(
            report.contains("process table unavailable: ps: exit 1"),
            "a failed ps must name itself"
        );
        assert!(
            report.contains("NOT an empty fleet"),
            "and must not read as a clean, empty board"
        );
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
