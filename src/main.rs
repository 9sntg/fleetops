//! Fleetops — CLI entrypoint: launch the board or run doctor.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/main.rs
//! Deps:    tokio (explicit runtime); tui, doctor, runner
//! Tested:  n/a (thin shell; all seams tested in their modules)
//!
//! Key responsibilities:
//! - Dispatch: (default) tui board | `--no-highlight` (board, tints off) | doctor | snapshot
//!   (headless JSON one-shot). Hand-rolled (house style, no clap).
//! - Report a fatal error on stderr with exit 1; unknown command exits 2.
//!
//! Design constraints:
//! - `unsafe_code = "forbid"` is crate policy (Cargo `[lints]`).

mod board;
mod cmux;
mod codex;
mod collect;
mod discovery;
mod doctor;
mod error;
mod fold;
mod git;
mod highlight;
mod paths;
mod procsrc;
mod runner;
mod snapshot;
mod telemetry;
mod tui;

use std::process::ExitCode;

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("fleet: cannot start runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match command.as_deref() {
        None => run_board(&runtime, true),
        Some("--no-highlight") => run_board(&runtime, false),
        Some("doctor") => {
            let (report, scan_ok) = runtime.block_on(doctor::run(&runner::Exec));
            println!("{report}");
            if scan_ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE // spec 004: exit 1 when the scan itself failed
            }
        }
        Some("snapshot") => {
            let (json, scan_ok) = runtime.block_on(snapshot::run(&runner::Exec));
            println!("{json}");
            if scan_ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE // spec 009: exit 1 when the scan itself failed
            }
        }
        Some(other) => {
            eprintln!(
                "fleet: unknown command '{other}' (usage: fleet [--no-highlight|doctor|snapshot])"
            );
            ExitCode::from(2)
        }
    }
}

fn run_board(runtime: &tokio::runtime::Runtime, highlight_enabled: bool) -> ExitCode {
    match runtime.block_on(tui::run(highlight_enabled)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fleet: {e}");
            ExitCode::FAILURE
        }
    }
}
