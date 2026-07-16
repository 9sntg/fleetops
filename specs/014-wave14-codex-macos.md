# Wave 14 — the Codex lane on macOS

> The Codex lane walked `/proc`, so it returned empty on macOS. Port it to `ps` + `lsof`.
>
> Status: **Draft** (agents never promote to Active — the maintainer does).
> The maintainer chose to keep this lane (2026-07-16) and installed Codex CLI so it could be
> verified rather than written blind. That decision paid for itself immediately — see below.

## Goal

Live Codex TUI sessions appear on the board again, on macOS.

## Why this could not be written blind

The Linux gate is `comm == "codex"`, reading `/proc/<pid>/comm` — the **bare basename**.

macOS `ps` reports comm/argv0 as the **full path**. Captured live 2026-07-16 from a running TUI:

```
51450  /Users/…/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex   <- real binary
51448  node /Users/…/.nvm/versions/node/v24.18.0/bin/codex                          <- node shim
```

`comm == "codex"` is therefore **false for every process, forever**. The lane would have found
nothing, returned an empty vec, and rendered exactly like "no Codex is running" — a silent,
total failure that unit tests over synthetic Linux-shaped data would have happily passed.

Worse: the node shim shares the real binary's **tty AND cwd**. Since the rollout join refuses to
join two processes sharing a cwd (never guess — house rule), failing to exclude the shim would
have dropped the real session too.

## Data contract (verified live 2026-07-16, against a real authenticated session)

| Need | Linux | macOS |
|---|---|---|
| recognize the TUI | `comm == "codex"` + argv0-only cmdline + `fd/1 -> /dev/pts/*` | **basename(argv0) == `codex`** + argv0-only + owns a tty |
| cwd (the join key) | `/proc/<pid>/cwd` readlink | **`lsof -a -d cwd -p <csv> -Fpn`** (macOS `ps` has no cwd field) |
| start time | `btime + starttime_ticks / HZ`, `HZ` hardcoded to 100 | **`now - ps etime=`** — no boot time, no HZ guess |
| tty / surface id | `/proc/<pid>/fd/1`, `/proc/<pid>/environ` | wave 11's process table (already fetched) |

The **rollout format is unchanged** — verified against a real rollout at
`~/.codex/sessions/2026/07/16/rollout-…jsonl`: `session_meta` line 0 with `payload.{id,cwd}`,
then `event_msg/{task_started,user_message,token_count,task_complete}` and `response_item`.
Two new envelope types appeared (`world_state`, `turn_context`) — the tolerant parser skips them,
which is exactly the drift-tolerance the lane was built for. `token_count` still carries
`info.total_token_usage.total_tokens` and `info.model_context_window`.

## Behaviour

1. **The gate** is `basename(argv0) == "codex"` AND argv is one token AND the pid owns a tty.
   Excludes the node shim (argv0 `node`), `codex exec …` and `--version` (extra argv), and any
   piped/daemonized codex (no tty).
2. **No Codex running costs no `lsof`** — the common case is one `ps` and an early return.
3. **The cwd join is one batched `lsof`**, never one per pid.
4. **`ps -Awwo pid=,args=` deliberately omits `-E`.** The Claude lane's table needs environments;
   this one must NOT have them: `ps -E` appends the environment to the command with no delimiter,
   so argv and environ become indistinguishable and the gate's token count — the whole
   recognizer — turns to nonsense. Omitting `-E` also keeps every process's secrets out of reach.
5. **Codex under cmux matches its surface** by `CMUX_SURFACE_ID`, exactly like Claude (wave 12),
   since both read the same process table.
6. A failed Codex lane costs its rows, never the board.

## Seams & structure

`codex::scan(codex_root, proc_root, …)` becomes `codex::scan(codex_root, proc_infos, surfaces)`
— pure over already-fetched processes. The new async lane mirrors the house four-layer split:

| Layer | Item |
|---|---|
| pure spec builders | `procs_spec()`, `cwds_spec(pids)` |
| pure parsers | `is_codex_tui(argv, tty)`, `parse_procs(bytes, tty_of)`, `parse_cwds(bytes)` |
| thin async fetch | `fetch(runner, table)` |

**Deleted:** `scan_procs`, `read_proc_info`, `read_btime`, the `HZ` constant, the `fake_codex_proc`
test tree — and with them `discovery::starttime_from_stat`, whose last caller this was. **No
`/proc` reference remains in `src/`.**

## Deterministic tests (red first)

- 🔴 `is_codex_tui` table over the REAL captured argv0 — plus
  `the_full_path_comm_is_why_linuxs_rule_could_not_be_reused`, the regression pin for the finding
  above.
- 🔴 `parse_procs` over `tests/fixtures/ps-codex-args.txt` (live: real binary + node shim + an
  `exec` form + noise) → only the real binary; no tty → nothing; garbage rows skipped.
- 🔴 `parse_cwds` over `tests/fixtures/lsof-cwd.txt` (live `-Fpn` output); an orphan `n`-line
  names no process; garbage → empty.
- 🔴 `procs_spec` never asks for `-E`; `cwds_spec` batches pids.
- 🔴 `fetch` skips `lsof` entirely when no Codex is running (`CannedRunner`, no spawn).
- 🔴 `procsrc::parse_etime` table — `MM:SS`, `HH:MM:SS`, `D-HH:MM:SS`, garbage.
- 🟢 `./check.sh` green (140 tests).

## Verified end-to-end (2026-07-16)

Against a real authenticated Codex session started in `/tmp`:

```
fleet snapshot →  [CODEX] 'hi in one word'  status='Idle'  tok=13081  cwd=/private/tmp
board          →  9  ✳ idle  🦀 tmp  hi in one word  █░░░░░░░░░  13k  codex  7m  —
footer         →  · 1 codex
```

Every link confirmed against reality: the gate picked the vendored binary over the shim, `lsof`
returned `/private/tmp`, that joined the rollout whose `session_meta.cwd` is `/private/tmp`, the
fold read `task_complete` → `Idle`, and `13081` matches the rollout's `token_count` exactly. The
name is the message actually typed.

## Out of scope

- `~/.codex/logs_2.sqlite` (present on macOS) would join exactly and retire the cwd heuristic —
  the recorded upgrade trigger if cwd-join ambiguity ever bites. Still not v1.
- Codex sessions started outside cmux show `—` in PANE, same as Claude.

## Dependencies

**None added.** `ps` + `lsof` via the existing `Runner` seam.
