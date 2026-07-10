//! The subprocess seam: a fully-specified command, a `Runner` trait, and its real executor.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/runner.rs
//! Deps:    tokio (process, time), async-trait
//! Tested:  inline `#[cfg(test)]` (Exec against `echo` + `false`); CannedRunner drives panes tests
//!
//! Key responsibilities:
//! - `CommandSpec`: an explicit argv + timeout, built by pure functions (never a shell string).
//! - `Runner`: the injectable seam so sensors are testable with canned bytes (no process spawn).
//! - `Exec`: the real `tokio::process` executor, bounded by `spec.timeout`, stdin nulled.
//!
//! Design constraints:
//! - Never build a shell string — always explicit argv (see rules/rust/subprocess-safety.md).
//! - Every external call is bounded by a timeout; a hung child must never freeze the loop.
//! - Ported from tokenomics src/runner.rs (proven, same house style); env overrides dropped
//!   until a sensor needs them.

use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;

use crate::error::{AppError, AppResult};

/// A fully-specified external command. Built by pure argv builders, executed by a [`Runner`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// The program to run (argv[0]); resolved via `PATH`.
    pub program: String,
    /// The arguments (argv[1..]); explicit, never shell-parsed.
    pub args: Vec<String>,
    /// Environment overrides layered onto the inherited environment (e.g. the WSLENV-forwarded
    /// `WEZTERM_UNIX_SOCKET` that targets one specific wezterm instance).
    pub env: Vec<(String, String)>,
    /// Hard per-call timeout.
    pub timeout: Duration,
}

/// The injectable subprocess seam. `Exec` runs real processes; tests use a canned runner.
#[async_trait]
pub trait Runner: Send + Sync {
    /// Run `spec`, returning its stdout bytes on success. Bounded by `spec.timeout`.
    async fn run(&self, spec: &CommandSpec) -> AppResult<Vec<u8>>;
}

/// The real executor over `tokio::process`, bounded by `spec.timeout`.
#[derive(Debug, Default, Clone, Copy)]
pub struct Exec;

#[async_trait]
impl Runner for Exec {
    async fn run(&self, spec: &CommandSpec) -> AppResult<Vec<u8>> {
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.args(&spec.args);
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }
        // No stdin: a child that waits on input must never wedge a sensor.
        cmd.stdin(Stdio::null());
        // Reap a timed-out child: on timeout the future is dropped, and kill_on_drop kills the
        // process rather than leaking a zombie wezterm.exe on every slow poll.
        cmd.kill_on_drop(true);

        let output = match tokio::time::timeout(spec.timeout, cmd.output()).await {
            Err(_elapsed) => {
                return Err(AppError::Timeout {
                    program: spec.program.clone(),
                    seconds: spec.timeout.as_secs(),
                })
            }
            Ok(Err(spawn_err)) => {
                return Err(AppError::Subprocess {
                    program: spec.program.clone(),
                    message: spawn_err.to_string(),
                })
            }
            Ok(Ok(output)) => output,
        };

        if output.status.success() {
            return Ok(output.stdout);
        }
        Err(AppError::Subprocess {
            program: spec.program.clone(),
            message: exit_summary(output.status.code(), &output.stderr),
        })
    }
}

/// A short, secret-free failure summary: the exit code and the last few stderr lines.
fn exit_summary(code: Option<i32>, stderr: &[u8]) -> String {
    let code = code.map_or_else(|| "signal".to_string(), |c| c.to_string());
    let text = String::from_utf8_lossy(stderr);
    let mut tail: Vec<&str> = text.lines().rev().take(3).collect();
    tail.reverse();
    if tail.is_empty() {
        format!("exit {code}")
    } else {
        format!("exit {code}: {}", tail.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(program: &str, args: &[&str]) -> CommandSpec {
        CommandSpec {
            program: program.to_string(),
            args: args.iter().map(|s| (*s).to_string()).collect(),
            env: Vec::new(),
            timeout: Duration::from_secs(5),
        }
    }

    #[tokio::test]
    async fn exec_returns_stdout_on_success() {
        let out = Exec
            .run(&spec("echo", &["hello"]))
            .await
            .expect("echo runs");
        assert_eq!(String::from_utf8_lossy(&out).trim(), "hello");
    }

    #[tokio::test]
    async fn exec_surfaces_nonzero_exit_as_subprocess_error() {
        let err = Exec
            .run(&spec("false", &[]))
            .await
            .expect_err("false exits 1");
        assert!(matches!(err, AppError::Subprocess { .. }));
    }

    #[tokio::test]
    async fn exec_reports_missing_program() {
        let err = Exec
            .run(&spec("fleet-no-such-program-xyz", &[]))
            .await
            .expect_err("missing program");
        assert!(matches!(err, AppError::Subprocess { .. }));
    }

    #[test]
    fn exit_summary_includes_code_and_stderr_tail() {
        let s = exit_summary(Some(2), b"line one\nline two\n");
        assert!(s.contains("exit 2"));
        assert!(s.contains("line two"));
    }
}

/// A test seam that returns canned outputs (in order, last repeats) and records every spec it
/// was handed — so sensor parsing is tested with no process spawn. Test-only.
#[cfg(test)]
#[derive(Debug)]
pub struct CannedRunner {
    outputs: Vec<AppResult<Vec<u8>>>,
    calls: std::sync::Mutex<Vec<CommandSpec>>,
}

#[cfg(test)]
impl CannedRunner {
    /// Build a runner that always returns `output` and records each spec.
    pub fn new(output: impl Into<Vec<u8>>) -> Self {
        Self {
            outputs: vec![Ok(output.into())],
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Build a runner that returns `outputs` in call order (the last one repeats).
    pub fn new_seq(outputs: Vec<AppResult<Vec<u8>>>) -> Self {
        assert!(!outputs.is_empty(), "need at least one canned output");
        Self {
            outputs,
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// The most recent spec passed to `run`, if any.
    pub fn last_spec(&self) -> Option<CommandSpec> {
        self.calls
            .lock()
            .expect("canned runner mutex")
            .last()
            .cloned()
    }

    /// Every spec passed to `run`, in call order.
    pub fn all_specs(&self) -> Vec<CommandSpec> {
        self.calls.lock().expect("canned runner mutex").clone()
    }
}

#[cfg(test)]
#[async_trait]
impl Runner for CannedRunner {
    async fn run(&self, spec: &CommandSpec) -> AppResult<Vec<u8>> {
        let mut calls = self.calls.lock().expect("canned runner mutex");
        calls.push(spec.clone());
        let index = (calls.len() - 1).min(self.outputs.len() - 1);
        match &self.outputs[index] {
            Ok(bytes) => Ok(bytes.clone()),
            Err(_) => Err(AppError::Subprocess {
                program: spec.program.clone(),
                message: "canned failure".to_string(),
            }),
        }
    }
}
