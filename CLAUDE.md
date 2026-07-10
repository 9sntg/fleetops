# Fleetops

Fleetops is a single-binary Rust TUI that monitors **all running Claude Code sessions** on this
machine — the fleet. Per session it shows a **semantic name** (what the session is working on),
**status** (working / done / needs input / question), **tokens spent**, **context % remaining**
(same numbers as the status bar), and the wezterm pane it lives in (jump-to-pane). Sessions run
across one wezterm window with many tabs/panes; fleetops renders the overview on the TUI monitor.
Built for WSL2. Sibling of `/tui/tokenomics` (accounts/limits) — fleetops is per-**session**,
tokenomics is per-**account**.

## Status

**Waves 1–4 shipped** (specs 001–004) — full Option A "sensor fusion" per the dossier in
`plans/001-*`: session discovery (`~/.claude/sessions/*.json` + /proc liveness + account
attribution), transcript-tail telemetry (ctx%/tokens/ai-title/pending question), pure status
fold (NeedsAnswer / Waiting / Stalled? / Unknown / Working / Idle / Shell), title-first pane
matching + jump, `fleet doctor` drift report. Verified data sources + implementation
corrections: `docs/RESEARCH.md`. Next (trigger-gated, see dossier): hook lane, SQLite history,
WSLENV pane forwarding.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust (2021, strict — `forbid(unsafe_code)`, clippy pedantic `-D warnings`) |
| TUI | ratatui + crossterm |
| Async | tokio (never block the UI task; results arrive as messages over channels) |

## Commands

```bash
./check.sh              # THE GATE: fmt --check + clippy -D warnings + test (must be green)
fleet                   # launch the board (~/.local/bin/fleet -> target/release/fleet)
fleet doctor            # read-only drift report (sessions/transcripts/panes/wezterm)
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
