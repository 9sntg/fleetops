# Fleetops — verified data sources (recon 2026-07-10)

Facts checked on this machine before architecture. Each row was verified live, not assumed.

## What the tool must show, and where it comes from

| Need | Source | Verified |
|---|---|---|
| Status: working / done / needs input | Claude Code hooks already fire per session: `UserPromptSubmit → working`, `Notification → input`, `Stop → done`, `SessionEnd → clear` via `~/.claude/helpers/claude-wezterm-status.sh` (sets wezterm user var `CLAUDE_STATUS` over OSC 1337 on `$WEZTERM_PANE`) | ✅ script read |
| Context %, cost, tokens, model | `~/.claude/helpers/statusline.mjs` — statusline command receives full JSON every render tick | ✅ exists |
| Token usage, cwd, git branch, message history | Session transcripts: `~/.claude-acct/<acct>/projects/<cwd-slug>/<session-uuid>.jsonl` — entries carry `sessionId`, `cwd`, `gitBranch`, usage, `stop_hook_summary`, `turn_duration` | ✅ tail-read |
| Semantic session name | Claude Code writes `summary` entries into the session JSONL; fallback: one cheap Haiku call per session. **No embeddings needed** | partially (summary entries known from format; verify per-version) |
| Pane mapping / jump-to-session | `wezterm.exe cli list --format json` (Windows wezterm, works from WSL); `activate-pane` to jump | ✅ binary at `/mnt/c/Program Files/WezTerm/wezterm.exe` |

## Constraints discovered

- **Multiple accounts**: sessions live under `~/.claude-acct/*/projects/` (gmail seen; others likely) —
  fleet discovery must scan all account dirs, not one.
- Statusline runs only on render ticks → an idle session's numbers go stale; hooks cover the
  transitions (done/needs-input), so staleness only affects tokens/context of idle sessions.
- Hook script writes to `/dev/tty` (wezterm user var), not to any file — a file/store lane for
  fleetops would be an **addition** (extra hook arg or second hook), not a replacement.
- `claude-wezterm-status.sh` no-ops outside wezterm (`$WEZTERM_PANE` guard).

## Prior art in `/tui`

- `tokenomics` — per-account usage/limits TUI (Rust + ratatui + tokio + rusqlite WAL; collector
  writes, TUI reads). Fleetops decree: same shape, per-session instead of per-account.
- `ghmonitor`, `ground-control`, `bridge` — sibling TUIs; check for reusable patterns.
