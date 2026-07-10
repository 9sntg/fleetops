# Fleetops — verified data sources

> Recon 2026-07-10 on this machine (Claude Code v2.1.206). Superseded in depth by
> `plans/001-fleetops-architecture/01-deep-dive.md` (D1–D9 + assumption log) — this file is the
> short index. Initial version contained errors, corrected below after live verification.

## What the tool must show, and where it comes from

| Need | Source | Status |
|---|---|---|
| Live-session discovery + busy/idle + name | **`~/.claude/sessions/<pid>.json`** — Claude Code natively maintains `{pid, sessionId, cwd, procStart, name, status: busy\|idle\|shell, updatedAt}`. ⚠️ leaves stale files for dead PIDs → liveness = `/proc/<pid>` + procStart match | ✅ verified (KEY FINDING) |
| Tokens, context %, semantic title, pending question | Transcript JSONL `~/.claude/projects/<slug>/<uuid>.jsonl`: `usage` on assistant lines (ctx% recipe = input+cache_read+cache_creation vs 200k/1M), **`ai-title` entries** (NOT `summary` — those don't exist in v2.1.x), pending `AskUserQuestion` tool_use. Permission prompts are **never** in JSONL | ✅ verified |
| Instant transitions + permission prompts | Hooks (`async:true`): UserPromptSubmit carries `{prompt, session_title}`, Notification `{notification_type: permission_prompt\|idle_prompt, message}`, Stop `{last_assistant_message}`, SessionEnd. No hook covers AskUserQuestion (anthropic #13024) | ✅ payloads binary-verified |
| Pane mapping / jump | `wezterm.exe cli list --format json` (median 110 ms from WSL): pane `title` already carries Claude's live title + status glyph (⠂ working / ✳ idle), `cwd` matches session cwd. `activate-pane` to jump. No user-vars readable via CLI | ✅ verified |
| Account attribution | `/proc/<pid>/environ` → `CLAUDE_CONFIG_DIR`, `CLAUDE_ACCOUNT` (transcript paths carry no identity) | ✅ verified |
| Fallback name | `~/.claude/history.jsonl` `{display, sessionId, project, timestamp}` — free; Haiku call (~$0.0011) only as last resort | ✅ verified |

## Corrections vs initial recon (2026-07-10)

1. **`WEZTERM_PANE` never reaches WSL** (WSLENV forwards only TERM vars) — the existing
   `claude-wezterm-status.sh` hook chain is a verified **no-op** on every session (guard exits).
   The wezterm tab icons the maintainer configured have never fired from WSL sessions.
2. **One unified store**: every `~/.claude-acct/<acct>/{projects,sessions}` is a symlink to
   `~/.claude/{projects,sessions}` — fleet discovery scans ONE dir; multi-account is a non-problem
   for discovery (but attribution needs /proc).
3. **No `"type":"summary"` entries** exist (checked all 3,866 transcripts) — semantic names come
   from `ai-title` entries / `history.jsonl` / hook `session_title`.
4. **Statusline input JSON** (official docs 2026-07-09) now includes `context_window.used_percentage`,
   `rate_limits`, `session_name` — richer than assumed; local `statusline.mjs` still self-computes.

## Scale facts

3,866 transcript JSONLs total; ~42 modified/24 h; ~17 concurrent sessions typical; sizes p50 214 KB,
p90 732 KB, max 22.3 MB. All on ext4 (inotify reliable; watch dirs not files). `usage.output_tokens`
is a mid-stream snapshot (undercounts ~2×, #27361) — tokens are approximate, never billing-truth.

## Prior art in `/tui`

- `tokenomics` — per-account (fleetops = per-session); collector+SQLite+MVU patterns to port.
- `bridge` — **hub pattern**: many async source tasks → normalized events → one stream → TUI; ndjson codec.
- `ground-control` — same MVU + subprocess-safety shape. `ghmonitor` — Go, UX reference only.
