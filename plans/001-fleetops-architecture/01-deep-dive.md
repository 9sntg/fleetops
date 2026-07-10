# Deep dive — codebase & data-source verification

> Evidence pack, verified 2026-07-10 on this WSL2 machine (Claude Code v2.1.206, 6 accounts,
> 17 live sessions at recon time). Sources: live commands, file reads, binary grep, official docs.
> Confidence tags: **verified** (observed output), **reported** (dated external source), **inferred**.

## Current state (codebase)

Fleetops is greenfield — no `src/` yet. The decreed reference is `/tui/tokenomics`; siblings
`/tui/{ground-control,bridge,ghmonitor}` were scanned for reuse.

### Tokenomics patterns (the decree "same shape")

| Pattern | Mechanism | Source |
|---|---|---|
| Collector↔TUI split | Two OS processes coordinate ONLY via shared SQLite (WAL: 1 writer + N readers, no IPC/locks) | `tokenomics/src/main.rs:318-351,426-449`, `store.rs:103-108` |
| Single-writer discipline | Collector loop task solely owns `Store`; subprocess/network work runs as tokio tasks returning outcome structs to the loop, which alone writes | `collector.rs:20-26,127-182,356-401` |
| Store | 5-table schema, `PRAGMA user_version` migrations (idempotent, additive), WAL + `busy_timeout=5000` + `synchronous=NORMAL`, hourly retention prune + `wal_checkpoint(TRUNCATE)`, heartbeat table | `store.rs:32-145,381-423` |
| MVU seams | `tui/model.rs` (App + pure `update(Msg)`, `now` injected) / `view.rs` (pure render) / `keys.rs` (key→Action table) / `mod.rs` (ONLY I/O: terminal + `tokio::select!` loop) | `tui/*.rs`, `rules/rust/ratatui-architecture.md:25-58` |
| Event loop | One `select!` over crossterm EventStream / 1s interval tick (re-read store → `Msg::Data(Box<…>)`) / ctrl-c; `MissedTickBehavior::Skip`; `try_init/try_restore` panic hook | `tui/mod.rs:44-103`, `model.rs:214-291` |
| Subprocess safety | `runner.rs`: pure argv builders → `CommandSpec{program,args,env,timeout}`; `Runner` trait seam (+`CannedRunner` for tests); `Exec` = tokio process, `stdin=null`, `kill_on_drop`, `time::timeout`, secret-free stderr tail | `runner.rs:25-100` |
| Paths/config | `paths.rs` cwd-independent (env override else XDG); TOML `deny_unknown_fields`; pure `validate()` split from `validate_environment()` | `paths.rs:21-72`, `config.rs:25-239` |
| Provider seam | `#[async_trait] ProviderAdapter::collect(...) -> AppResult<Option<UsageSnapshot>>` (None = idle, not error) | `providers/mod.rs:22-30` |
| Tests | 127 test attrs; every seam pure or canned (no spawn/network); fixtures via `include_bytes!`; insta TUI snapshots | grep + `docs/handoff/2026-07-05` |

### Tokenomics pain points (lessons paid for)

Six staleness bugs, all of the class **"stale data shown as live"** (`CHANGELOG.md:55-101`, spec 011/012):
heartbeat written but never read; idle accounts frozen at last value; stale authoritative overlay winning merge;
overlay awaited inline froze the loop; unbounded GROUP BY slowing over time; plus two ops footguns —
**stale installed binary** (`cargo run` vs `~/.local/bin/tok`) and **shared `projects/` symlink** making
per-account numbers identical (accepted as aggregate-only; fleet reducers use max-not-sum, `model.rs:137-166`).

### Siblings

- **ground-control** (Rust/ratatui, bin `gc`): same MVU + paths/config shape; subprocess safety with `nix` signals. No Claude/wezterm code.
- **bridge** (Rust/ratatui, bin `br`): **most relevant** — `hub.rs` central event hub; each `sources/*.rs` is a tokio task decoding provider output into normalized `Event` → hub → TUI ("many async lanes → one stream"); `codec/ndjson.rs` JSONL decoding directly reusable.
- **ghmonitor** (Go/Bubble Tea): UX pattern only; WSL clipboard lane (`clip.exe`/OSC52) worth mirroring.

## Constraints

- Stack decreed: Rust 2021, `forbid(unsafe_code)`, clippy pedantic `-D warnings`, ratatui + crossterm + tokio. Branch: `main` only.
- Fleetops is **read-only over the fleet** (CLAUDE.md Never-list); writing into Claude config dirs is **Ask-first**.
- Spec-driven TDD; `check.sh` gate; rules/ ported.
- Solo user, one machine; ops surface must be near-zero (attention is the scarce resource).

## Data-source verification (the heart of the decision)

### D1 — Native session state files: `~/.claude/sessions/<pid>.json` ⭐ KEY FINDING

Claude Code v2.1.x **natively maintains** per-session state files:
`{pid, sessionId, cwd, startedAt, procStart, version, kind:'interactive', entrypoint, name, nameSource:'derived', status:'busy'|'idle'|'shell', updatedAt, statusUpdatedAt}`.
36 present; busy sessions update within seconds. **Live discovery + coarse status with zero installation.**
⚠️ 20 of 36 referenced **dead PIDs** (crash leftovers — Anthropic's own hook-push has the stale-file problem).
Reliable liveness = `/proc/<pid>` exists AND starttime matches `procStart` (PID-reuse guard).
Source: `cat ~/.claude/sessions/459060.json`; alive/dead loop, 2026-07-10. **Undocumented internal — may change any release (ASSUMPTION A1).**

### D2 — Unified transcript store (multi-account problem dissolved)

Every `~/.claude-acct/<acct>/projects` AND `<acct>/sessions` is a **symlink to `~/.claude/{projects,sessions}`** —
one dir covers all 6 accounts. Account attribution must come from `/proc/<pid>/environ` (`CLAUDE_CONFIG_DIR`,
`CLAUDE_ACCOUNT` readable — verified across 17 live PIDs). RESEARCH.md's per-account scan premise was wrong.

### D3 — Transcripts (JSONL): tokens, context %, names, questions

- Location `~/.claude/projects/<cwd-slug>/<uuid>.jsonl`; 3,866 files total, **42 modified in 24h** (fleet-size proxy); sizes p50 214 KB, p90 732 KB, max 22.3 MB. Subagent transcripts live separately under `<uuid>/subagents/…` (`isSidechain:true` only there).
- **Context %**: NOT delivered anywhere push-style per tick with history; recipe (used by `statusline.mjs:92-116`): last assistant line's `usage.input_tokens + cache_read_input_tokens + cache_creation_input_tokens` vs 200k/1M window.
- **Names**: NO `"type":"summary"` entries anywhere (RESEARCH.md stale). Instead: `"type":"ai-title"` entries `{aiTitle, sessionId}` (8–15 per file, take last); `slug` on some assistant lines (absent in some files); `~/.claude/history.jsonl` `{display, sessionId, project, timestamp}` = free always-fresh prompt text.
- **Questions**: a pending `AskUserQuestion` tool_use IS visible in JSONL; **permission prompts are NOT written to JSONL at all**.
- Format: per-line `version` field; 9 CLI versions in 30 days; undocumented line types (`last-prompt`, `mode`, `attachment`) — parser must match-needed-fields and skip unknown types. Officially "internal, changes between versions" (code.claude.com/docs/en/sessions.md).
- `usage.output_tokens` is a mid-stream snapshot (undercounts ~2×, anthropics/claude-code#27361) — token figures are approximate, never billing-truth.
- Compaction appends in-place (`isCompactSummary:true`); file NOT truncated → byte-offset tailing survives `/compact`.

### D4 — Hooks (the push lane)

- All 7 settings.json files carry identical hooks today → `claude-wezterm-status.sh` (OSC user var to `/dev/tty`, never a file). **BUT: `WEZTERM_PANE` never reaches WSL** (WSLENV forwards only TERM vars) → **the existing status hook is a verified no-op** on every session. RESEARCH.md corrected.
- Hook stdin (binary-verified, v2.1.206): base `{session_id, transcript_path, cwd, prompt_id, permission_mode, agent_id, agent_type, effort}` + per-event: `UserPromptSubmit={prompt, session_title}`; `Notification={message, title, notification_type}`; `Stop={stop_hook_active, last_assistant_message}`; `SessionEnd={reason}`; `SessionStart={source, model, session_title}`. **`session_title` arrives via hooks — semantic name without transcript reads.**
- Notification types: `permission_prompt`, `idle_prompt`, +6 others. **Gaps (official tracker)**: no hook covers AskUserQuestion (#13024 open); `idle_prompt` has hardcoded 60s delay, false-positive/missing-field reports (#12048, #8320). Headless `claude -p`: hooks unreliable (#40506, #38651).
- `async: true` command hooks = fire-and-forget (no UX latency, no error spam); measured spawn cost `bash -c true` ≈ 2.2 ms.
- Drift is real: the 7 settings.json show 5 distinct md5s today. Hook lane ⇒ needs installer/verifier (`doctor`), and settings edits are Ask-first per CLAUDE.md.

### D5 — Statusline tap

Official statusline stdin JSON (docs 2026-07-09) is rich: `context_window.{used_percentage, remaining_percentage, total_input_tokens, context_window_size, current_usage.*}`, `cost.total_cost_usd`, `rate_limits.{five_hour,seven_day}`, `session_id`, **`session_name`**, `transcript_path`, `model.*`, `effort.level`. A one-line tee in `statusline.mjs` could dump this per session per render tick. ⚠️ Fields may be null before first API call / after `/compact`; runs only while a session renders (no push on idle); current `statusline.mjs` computes context % itself from the transcript (`:50-55,92-116`).

### D6 — wezterm pane mapping

- `wezterm.exe cli list --format json` works from WSL: per-pane `pane_id, tab_id, title, cwd, tty_name, is_active…`. Measured: **median 110 ms, max 260 ms** (exe on C:\, fast interop case) — 1–2 s polling fine in its own tokio task with timeout.
- **Pane titles already carry Claude's live semantic title + status glyph** (`⠂ Brainstorm monitoring tool…` working / `✳ …` idle) — Claude Code stamps them via OSC and users can't disable (#31107). Free coarse status + name + pane match, but format undocumented (ASSUMPTION A2).
- NO user-vars in CLI output and no verb to read them → the OSC user-var lane is unreadable externally even if it worked.
- Pane↔session mapping keys: pane `cwd` (`file://wsl.localhost/Ubuntu/...`) × session cwd, plus title×ai-title match. Exact mapping possible later by forwarding `WEZTERM_PANE` through WSLENV (Windows-side env change, out of v1 scope).
- WSL interop context: pathological spawn reports exist (powershell ~700 ms, #29672); wezterm.exe measured fast; keep argv+timeout runner seam.

### D7 — Processes

`pgrep -af 'claude --dangerously'` → 17 live (16 top-level); `/proc/<pid>/environ` readable → per-session `CLAUDE_CONFIG_DIR`/`CLAUDE_ACCOUNT` (account attribution), `/proc/<pid>/stat` → starttime for PID-reuse guard. State R/S + CPU% = working-vs-idle only; confounded by idle-at-100%-CPU bugs (#19393).

### D8 — Filesystem/inotify

`~/.claude`, `/tui` on ext4 (`/dev/sdd`) — native inotify reliable (WSL#4739 applies only to /mnt drvfs).
`max_user_watches=524288`. Watch per-directory, not per-file. notify crate: Linux backend "not 100% reliable"
at large watch counts → PollWatcher fallback; partial-line reads = buffer to last `\n`.

### D9 — Naming via LLM (fallback lane)

Haiku 4.5 sync: ~$0.0011/title, 1–2 s; worst case this machine ≈ $0.17/day at 159 sessions/day. Batch −50% is wrong lane (latency). Any such runs must land in run-evidence per global CLAUDE.md. Built-in title generation is itself version-fragile (#29335: broke on deprecated model reference).

## Negative results (absence = signal)

- No official JSONL schema doc; no machine-readable session index (`sessions-index.jsonl` absent here); no `--list-sessions` CLI.
- No `"type":"summary"` entries in any of 3,866 transcripts (v2.1.198–206).
- No SQLite anywhere in `~/.claude` — Anthropic's own state plane is flat JSON files + JSONL.
- No wezterm CLI verb to read pane user vars.
- No sibling in `/tui` already watches Claude sessions or wezterm panes.
- No tool found that derives AskUserQuestion-pending purely from JSONL *except* generic pending-tool_use tracking; permission prompts invisible to JSONL, period.

## Assumption log

| # | Assumption | Why unverified | Impact if wrong |
|---|---|---|---|
| A1 | `~/.claude/sessions/<pid>.json` schema/behavior persists across CC versions | Undocumented internal (v2.1.198–206 observed) | Discovery/status lane degrades → fall back to process scan + JSONL mtime |
| A2 | wezterm pane-title glyph convention (⠂/✳ + title) stays recognizable | Emitting code not traced; #31107 says users can't disable, not that format is stable | Pane-lens status degrades; title×cwd match still works |
| A3 | Binary-grepped hook stdin fields = runtime schema | Minified strings, not docs | Hook payload parse breaks → hooks lane re-verified per version by `doctor` |
| A4 | 42 JSONLs/24h ≈ fleet scale; 17 concurrent sessions typical | Point-in-time counts | Watch-set sizing; ×10 still trivial for inotify limits |
| A5 | statusline stdin includes documented `context_window`/`rate_limits` fields on v2.1.206 | Docs dated 2026-07-09; local script predates them and self-computes | Statusline tap gets poorer → keep transcript recipe as primary |
| A6 | Native `status:'busy'` covers waiting-on-permission (not distinguishable) | Not probed during a live permission prompt | needs-input detection relies on hooks/JSONL lanes regardless |
