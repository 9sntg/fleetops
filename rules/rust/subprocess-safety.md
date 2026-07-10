---
rule: rust/subprocess-safety
title: Subprocess & Signal Safety
category: rust
scope: [rust, process]
priority: required
applies-to: [rust]
tags: [tokio-process, systemd, signals, pgid, timeouts, parsing]
---

# Subprocess & Signal Safety

**Enforcement**: code review + unit tests on every argv builder and every output parser.

---

## Core Principle

> This tool's whole job is driving other processes. Every external call is untrusted input at the boundary: build argv explicitly, bound it with a timeout, and parse its output defensively.

Ground Control shells out to `systemd-run`, `systemctl --user`, `journalctl`, `ss`, `lsof`, `fuser`, and health HTTP. Treat each as an integration boundary (see [[mock-boundaries]]).

---

## Never build a shell string

Always pass an explicit argv vector to `tokio::process::Command`. No `sh -c "..."`, no string interpolation of ports/paths/names into a command line.

```rust
// GOOD — argv, no shell, no injection surface
Command::new("systemctl").args(["--user", "stop", &unit_name(name)])

// BAD — shell parsing, injection, quoting hell
Command::new("sh").arg("-c").arg(format!("systemctl --user stop svc-{name}"))
```

`unit_name`, `port`, and `dir` come from config, but still never interpolate them into a shell — the argv boundary is the guarantee.

## The argv builder is a pure, tested function

Separate *constructing* the command from *running* it. A function that returns `Vec<String>` (or `Vec<OsString>`) is unit-testable without touching the OS:

```rust
fn systemd_run_args(spec: &ServerSpec) -> Vec<String> { /* ... */ }

#[test]
fn injects_reserved_port_and_workdir() {
    let a = systemd_run_args(&projectx());
    assert!(a.contains(&"--setenv=PORT=6555".into()));
    assert!(a.windows(2).any(|w| w == ["--working-directory", "/home/user/work/projectx"]));
}
```

This is how Wave 2's supervisor stays testable without a live systemd.

## Everything external gets a timeout

Wrap every spawned command and every health request in `tokio::time::timeout`. A hung `ss`, a wedged health endpoint, or a `journalctl` that blocks must never freeze the UI loop. On timeout, surface a state (`UNHEALTHY`/unknown), never hang.

## Reap children; prefer cgroup kills

- With the **systemd-user** backend, lifecycle is systemd's job — start via `systemd-run --user --unit=… --collect`, stop via `systemctl --user stop` (whole cgroup, SIGTERM→SIGKILL via `KillMode=control-group` + `TimeoutStopSec`). No orphan/zombie handling needed.
- In the **bare** fallback, spawn with a new session/process-group (`setsid`/`Setpgid`) and kill the **negative pgid** so the whole tree dies (multi-process dev servers like `next`+`convex` escape a single-PID kill). Use `nix::sys::signal::killpg` — a safe wrapper, no `unsafe` (see [[strict-lints]]).
- Send `SIGTERM`, wait up to the kill timeout, then `SIGKILL`. Re-check the port is free before declaring success.

## Parse output defensively

`ss`/`lsof`/`systemctl show` output is a contract that shifts between versions and locales:

- Match on stable machine-readable forms: `systemctl show -p Prop=Val` key/values (not human text); `ss -H -t -l -n -p` with explicit flags; prefer `-o cat` / null-delimited where available.
- Never assume a PID was found. `ss` without root **hides foreign-user PIDs** — "listening but no owner resolved" is a real, expected case that means *foreign / Windows-forwarded*, not *free*. Model it explicitly; advise, never blind-`sudo`.
- Tolerate multiple occupants on a port; operate on the whole set.
- Unit-test each parser against captured real fixtures (good, empty, multi-PID, no-owner).

## Boundary summary

| Call | Bound by | Parsed as | Failure surfaced as |
|---|---|---|---|
| `systemctl show` | timeout | key=val map | unit state = unknown |
| `ss`/`lsof`/`fuser` | timeout | PID set (may be empty/foreign) | port owner = none/foreign |
| health HTTP | timeout + insecure-TLS opt | status code | UNHEALTHY |
| `journalctl -f` | streamed, cancellable | tagged lines | log stream ends → mark closed |
