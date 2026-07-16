# fleetops

**An ops board for every Claude Code session running on your machine.**

Each Claude Code session is a ship; fleetops is the bridge's ops board — one terminal view of the
whole fleet: which sessions are working, which are idle, and which are blocked waiting on you, plus
tokens spent, context-window fill, and the cmux surface each one lives in (jump to any of them with a
keypress).

<!-- TODO: demo GIF here — record with docs/demo/board.tape (`vhs docs/demo/board.tape`) -->

## Features

- **Session discovery** — finds every live Claude Code session from `~/.claude/sessions/<pid>.json`,
  confirming liveness against `ps` (PID-reuse-safe: the file's recorded start time must match the
  process's actual start time). Stale files for dead PIDs are counted, never shown as live. A `ps`
  that fails is reported as a broken sensor, never as an empty fleet.
- **At-a-glance status** — a pure fold over each session's native status, pending-question flag, and
  transcript activity yields one of: **working**, **idle**, **needs answer** (a pending
  `AskUserQuestion`), **waiting** (blocked on input the transcript can't show, e.g. a permission
  prompt), **stalled?** (busy but the transcript stopped growing), **shell**, or **unknown** (a
  native status this build doesn't recognize — a drift signal, never hidden).
- **Tokens & context %** — reads the transcript tail for the last assistant `usage` line and renders
  a context-window gauge (out of 200k, or 1M once a session exceeds 200k) plus a compact token
  count. Approximate, never a bill.
- **cmux surface mapping & jump** — matches each session to its cmux surface by **exact identity**
  (`CMUX_SURFACE_ID`, read from the session process's own environment — never inferred from titles
  or cwd) and jumps to it on **Enter**, via cmux's AppleScript `focus`: one call that raises the
  window and focuses the tab.
- **Codex lane** — Codex CLI sessions (which keep no per-pid session file) are discovered from the
  process table + a batched `lsof` for their cwd, joined to their rollout transcript, and folded
  onto the same board.
- **Read-only over the fleet** — the only actions that change anything are focusing a surface (the
  jump) and an optional brief highlight of the jumped-to terminal (disable with `--no-highlight`).
  fleetops never writes into any Claude config or session directory.
- **`doctor` and `snapshot` subcommands** — `fleet doctor` prints a read-only drift report (are the
  undocumented sources still parseable?); `fleet snapshot` emits one JSON object of exactly what the
  board would render, for dashboards and scripts.

## Platform: macOS + cmux

fleetops targets **macOS**, with [cmux](https://cmux.com) as the terminal:

- Session discovery and liveness come from one `ps` call. macOS Claude Code records `procStart`
  as a UTC wall-clock string, and `TZ=UTC ps -o lstart=` reproduces it byte-for-byte, so the
  PID-reuse guard is a plain string comparison — fleetops never interprets the value.
- Pane identity is **exact**: cmux exports `CMUX_SURFACE_ID` into every terminal it starts, and
  the same `ps` call reads it back. No title or cwd guessing.
- Topology and the jump use cmux's shipped **AppleScript** interface, so no credential is
  involved and `fleet` works whether or not it runs inside cmux. macOS will ask once for
  Automation permission.

Sessions started in another terminal (Apple Terminal, iTerm2, …) still appear on the board with
full status, tokens and context — they simply show `—` in the PANE column and can't be jumped to,
because only cmux hands out a surface identity.

Without cmux running, the board still renders every session; `fleet doctor` reports
`⚠ cmux unreachable — jump lane degraded`.

> Earlier versions targeted WSL2 + wezterm. That lane is gone; see `specs/011`–`012`.

## Install

```bash
cargo build --release
# binary at target/release/fleet
```

Requires a recent stable Rust toolchain (see `rust-toolchain.toml`) and, for the jump lane,
[cmux](https://cmux.com) installed (macOS will prompt once for Automation permission).

## Quick start

```bash
fleet              # launch the board
fleet --no-highlight   # board, without the jump-target pane highlight
fleet doctor       # read-only diagnostics / drift report
fleet snapshot     # one-shot JSON of the current board, to stdout
```

Keys: **j/k** or **↑/↓** move the selection · **Enter** jumps to the selected session's cmux
surface · **r** refreshes · **q**/**Esc** quits.

## What it reads & privacy

Everything fleetops reads is local, and nothing is ever transmitted off the machine. It reads no
credentials — not tokens, not API keys. Specifically, per live session it reads:

- **`~/.claude/sessions/<pid>.json`** — pid, session id, cwd, native status, name, version.
- **`ps -AEwwo pid=,tty=,lstart=,command=`** — one call for the whole process table. From it:
  the **start time** (liveness / PID-reuse check), the **tty** (the target for the optional
  highlight), and exactly **two** environment variables — `CLAUDE_ACCOUNT` (account label) and
  `CMUX_SURFACE_ID` (exact surface identity). This allowlist is a security boundary: `ps -E`
  exposes process environments, which contain secrets (under cmux, `CMUX_SOCKET_CAPABILITY`),
  and nothing outside the two allowlisted names is ever captured, logged, or stored.
- **Transcript tail** (`~/.claude/projects/<slug>/<uuid>.jsonl`, last 256 KiB) — only **token
  counts**, the **ai-title**, and a **pending-question flag** are extracted. Message text is never
  read into state, logged, or stored.
- **cmux topology** — cmux's AppleScript interface, for surface ids / tab positions / cwd and the
  jump target. No cmux credential is read or used.
- **Codex** (only when a Codex TUI is running) — `ps -Awwo pid=,args=` (argv only, never
  environments), one batched `lsof -d cwd` for the rollout join key, and the rollout tail
  (`~/.codex/sessions/**/rollout-*.jsonl`) for status/tokens/name.

No data leaves your machine; fleetops makes no network requests.

## Unofficial

fleetops is an independent, unofficial tool. It is **not affiliated with, endorsed by, or supported
by Anthropic**. "Claude" is a trademark of Anthropic. It relies on undocumented, internal file
formats that can change at any time — `fleet doctor` exists to surface exactly that kind of drift.

## Maintenance

Passively maintained. Issues and PRs are welcome, but responses may be slow and features are added
only as they earn their keep.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions.
