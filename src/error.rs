//! Crate error type: every fallible boundary returns `AppResult`.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/error.rs
//! Deps:    thiserror
//! Tested:  exercised via runner/panes tests (no dedicated suite; variants are data)
//!
//! Key responsibilities:
//! - `AppError`: subprocess, timeout, parse, and terminal I/O failures.
//!
//! Design constraints:
//! - Messages carry no secrets (pane titles are user-visible task names, fine to include).
//! - No `unwrap`/`expect`/`panic!` in runtime paths — errors flow to the footer or exit.

use thiserror::Error;

/// Crate-wide result alias.
pub type AppResult<T> = Result<T, AppError>;

/// Everything that can go wrong at fleetops' boundaries.
#[derive(Debug, Error)]
pub enum AppError {
    /// An external command failed to spawn or exited non-zero.
    #[error("{program}: {message}")]
    Subprocess {
        /// `argv[0]` of the failing command.
        program: String,
        /// Short, secret-free failure summary.
        message: String,
    },

    /// An external command exceeded its timeout.
    #[error("{program}: timed out after {seconds}s")]
    Timeout {
        /// `argv[0]` of the timed-out command.
        program: String,
        /// The timeout that elapsed.
        seconds: u64,
    },

    /// External output did not parse as expected.
    #[error("parse: {0}")]
    Parse(String),

    /// Terminal / event-loop I/O failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
