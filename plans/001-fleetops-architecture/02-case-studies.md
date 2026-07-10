# Case studies & experience reports

> Prior-art sweep 2026-07-10. Question: how do existing tools monitor multiple concurrent
> Claude Code sessions, and what broke? Five data-plane families found; the robust tools hybridize.

## Taxonomy

| Lane | Tools | Essence |
|---|---|---|
| A. Pane-content scraping | claude-squad, claude-tmux | `tmux capture-pane` + string patterns |
| B. PTY wrap + patterns | ccmanager, happy (legacy), omnara (legacy) | own the terminal, parse output |
| C. Hooks → state push | tmux-claude-session-manager, claude-control, disler observability | event-driven state stamps |
| D. JSONL tailing | ccusage, Usage-Monitor, CCSeva, Stargx dashboard, sniffly, claude-devtools | rich token/cost data, laggy status |
| E. Process signals | tmux-claude (ethanpark374), claude-control fallback | ps state + CPU% |

## Per-tool findings

### claude-squad (lane A)
Polls `tmux capture-pane`, SHA256-diffs content for activity; waiting-state = hardcoded string
"No, and tell Claude what to do differently"; auto-answers trust prompts by string match. No token data.
**The brittle floor** — single UI-copy change breaks it silently. (source: session/tmux/tmux.go, fetched 2026-07-10)

### ccmanager (lane B — best-in-class no-hooks)
node-pty; per-tool strategy class per CLI with unit tests. Claude patterns: BUSY = "esc to interrupt"/spinner/token-stats line; WAITING = "Do you want"/"Would you like"+`❯` options; IDLE = 1500 ms unchanged debounce. Deliberately reads content *above* the prompt box to dodge redraw-race false positives (they got burned, engineered around it). Maintenance churn is real but absorbed: PR #298 (Apr 2026) added MCP-permission-prompt detection; #314 (Jun 2026) state filtering. **Pattern-scraping works but is a perpetual curation treadmill.**

### tmux-claude-session-manager / craftzdog (lane C)
Hooks stamp `@claude_state` (working/waiting/idle) onto tmux sessions at the moment of transition; "nothing polls in the background", "no separate database to get out of sync". Author explicitly rejected scraping/polling. (blog, June 15 2026)

### claude-control (lanes C+D+E hybrid, macOS)
`ps` discovery + auto-installed hooks writing `~/.claude-control/events/<pid>.json`; classification Working=UserPromptSubmit/SubagentStart, Waiting=PermissionRequest **but overridden if CPU>15%** (they distrust hooks against live CPU); JSONL mtime + CPU fallback when hooks absent. No token metrics. **Even hook-first tools keep a heuristic fallback.**

### happy / omnara (wrapper lane)
Only lane with *reliable immediate* permission interception — MCP permission server / SDK `canUseTool` — because they **own the launch**. Cost: must wrap `claude`, session restarts to adopt, protocol churn (happy #825: three protocol bugs). Omnara: state = textual analysis + explicit self-reported `requires_user_input` flag; HN pain: plaintext message visibility, GDPR.

### JSONL dashboards (lane D)
Stargx dashboard: chokidar-watch + tail-parse, infers thinking/waiting/idle from activity patterns, 2 s refresh, README admits inherent lag. ccusage/Usage-Monitor/CCSeva: token/cost excellence, no live status; CCSeva adds OAuth endpoint for server-truth limits. claude-powerline: primary = **native statusline stdin JSON** (rate_limits + usage), fallback transcript parse — validates the statusline-tap lane.
**JSONL parse cost is documented pain**: CC itself hangs at 99% CPU on large session files (#22041); ccusage_go exists specifically claiming −87% memory/−92% CPU vs the TS original; V8 GC thrash (#10479).

### Anthropic's own answer (agent teams, experimental)
`~/.claude/teams/<name>/config.json` + task list + TeammateIdle/TaskCompleted hooks + tmux panes. Documented pain: "task status can lag: teammates sometimes fail to mark tasks as completed". Machine-readable state files are their chosen shape too — but scoped to one lead session.

## The two KEY questions answered

**Q1 — Can "needs input / question asked" be derived WITHOUT hooks?**
Yes, three ways, all brittle: (1) output-pattern scraping (ccmanager grade = mature but perpetual curation; claude-squad grade = one string from breakage); (2) JSONL pending-`AskUserQuestion`-tool_use (works for questions; **permission prompts never hit JSONL**); (3) process signals (working-vs-idle only, confounded by idle-CPU bugs #19393).

**Q2 — Do hooks fully solve it?**
No. Anthropic's tracker: #13024 (open) — no WaitingForInput hook; Stop fires only on completion; `idle_prompt` is a 60 s timer with false positives (#12048) or no-fires (#8320); `permission_prompt` doesn't cover AskUserQuestion; "no current workarounds fully cover the AskUserQuestion case". Only launch-owning wrappers get deterministic interception. **Ceiling: without wrapping, 'question asked' = JSONL pending-tool_use (fast) + idle_prompt (slow confirm); 'permission needed' = Notification hook only.**

## PRAISE / PAIN summary per lane

- **Scraping** — PRAISE: zero setup, works on unowned sessions. PAIN: silent breakage on UI copy change, redraw races, per-pane polling, only visible screen.
- **Hooks** — PRAISE: event-driven, instant, no polling, survives detach. PAIN: settings.json mutation + drift, idle_prompt 60s/buggy, AskUserQuestion uncovered, headless `claude -p` unreliable (#40506), broken hook spams every session's UI (unless `async:true`).
- **JSONL** — PRAISE: richest data (tokens/context/cost/titles), retroactive, no ownership. PAIN: status lag, permission prompts invisible, CPU on large files, schema churn per release.
- **Process signals** — PRAISE: trivial, universal. PAIN: 2-state only, CPU-bug confounded.
- **Wrapper** — PRAISE: the only deterministic permission interception. PAIN: owns the launch (breaks the maintainer's plain-wezterm workflow), restart to adopt, protocol churn.

## Agentic-era note

All surveyed tools are 2025–2026 artifacts of exactly this workflow (many Claude Code sessions in terminal multiplexers). The stack fleetops decrees (Rust/ratatui/tokio, files+inotify, subprocess argv) is uniformly mainstream and agent-legible; the risky surface is not the tech but the **undocumented Claude Code internals** every one of these tools rides — mitigations are tolerant parsers, per-version `doctor` verification, and degraded modes.
