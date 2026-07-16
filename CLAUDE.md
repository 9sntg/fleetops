# Fleetops

Fleetops is a single-binary Rust TUI that monitors **all running Claude Code sessions** on this
machine — the fleet. Per session it shows a **semantic name** (what the session is working on),
**status** (working / done / needs input / question), **tokens spent**, **context % remaining**
(same numbers as the status bar), and the cmux surface it lives in (jump-to-surface). Sessions run
across cmux windows/tabs; fleetops renders the overview on the TUI monitor.
Built for macOS + cmux (waves 11-13 ported it off WSL2/wezterm). Sibling of `/tui/tokenomics` (accounts/limits) — fleetops is per-**session**,
tokenomics is per-**account**.

## Status

**Waves 1–10 shipped** on WSL2/wezterm (specs 001–010): session discovery, transcript-tail
telemetry (ctx%/tokens/ai-title/pending question), pure status fold (NeedsAnswer / Waiting /
Stalled? / Unknown / Working / Idle / Shell), pane matching + jump, `fleet doctor`, `fleet
snapshot`, the Codex lane. Verified data sources + implementation corrections: `docs/RESEARCH.md`.

**Waves 11–13 (specs 011–012) ported the whole thing to macOS + cmux** and deleted the Linux/WSL
sources rather than cfg-gating them:
- Liveness: one `TZ=UTC ps -AEwwo pid=,tty=,lstart=,command=` call replaces `/proc`. macOS Claude
  Code writes `procStart` as a UTC wall-clock string; `ps` reproduces it byte-for-byte, so the
  PID-reuse guard stays a string equality over an opaque token.
- Identity: `CMUX_SURFACE_ID` (from that same call's environ, allowlisted) → exact surface match.
  No title/cwd tie-breaks, no `ambiguous` outcome.
- Panes/jump: cmux's AppleScript API (`src/cmux.rs`), NOT its control socket — the socket is
  `cmuxOnly`-gated and would require handling `CMUX_SOCKET_CAPABILITY`, a credential.
- `src/panes.rs` (wezterm/WSL) is deleted.

**Wave 14 (spec 014) ported the Codex lane** to `ps` + a batched `lsof` cwd join, verified
against a real Codex session. Its Linux gate (`comm == "codex"`) was unusable: macOS `ps` reports
the full path, so the test was false for every process — the lane would have silently found
nothing. **No `/proc` reference remains in `src/`.**

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust (2021, strict — `forbid(unsafe_code)`, clippy pedantic `-D warnings`) |
| TUI | ratatui + crossterm |
| Platform | macOS only; `ps` for the process table, cmux's AppleScript API for panes/jump |
| Async | tokio (never block the UI task; results arrive as messages over channels) |

## Commands

```bash
./check.sh              # THE GATE: fmt --check + clippy -D warnings + test (must be green)
fleet                   # launch the board (~/.local/bin/fleet -> target/release/fleet)
fleet doctor            # read-only drift report (sessions/transcripts/surfaces/cmux)
cargo run               # dev build of the board
cargo build --release   # refresh the installed binary (the symlink tracks it)
```

## Rules

**Read before writing any code.** All coding rules live in `rules/`. Start at `rules/_index.md`;
route via `rules/crossroads.md`. Every `.rs` file carries a `//!` module header per
`rules/file-headers.md`. Rust specifics: `rules/rust/{strict-lints,ratatui-architecture,
subprocess-safety,async-tokio,error-handling,anti-patterns}.md`.

## Specs

**Development is spec-driven TDD.** One spec per wave in `specs/` (index: `specs/README.md`).
Cycle per wave: **spec → 🔴 red → 🟢 green → ♻ refactor-for-specs → ♻ refactor-for-rules**. Mark
ambiguities `[NEEDS CLARIFICATION]`; never guess.

## Versioning

- Maintain `CHANGELOG.md` `[Unreleased]` — entry for every user-facing change, same commit.
- Never bump the version or cut a release — only the user does.

## Git

- **Default branch: `main` — develop directly on `main`** (early-project decision, 2026-07-10).
  Introduce a `dev` branch only when the maintainer says so.

## Boundaries

- **Always**: run `./check.sh` green before calling a wave done. Follow `rules/`.
- **Ask first**: new external dependency; anything that writes into a Claude config/session dir.
- **Never**: `unsafe`. `unwrap`/`expect`/`panic!` in runtime paths. Log or print tokens/secrets
  from session transcripts. Mutate another session's files — fleetops is **read-only** over the fleet.
