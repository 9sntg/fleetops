---
plan: 003-cmux-poll-cost
status: active
owner: Santiago
created: 2026-07-19
type: investigation
---

# 003 — Why fleetops freezes cmux, and why the obvious fixes don't work

**Question (the operator, 2026-07-18):** Launching fleetops makes cmux unresponsive. Fix it —
without losing the workspace name in the STREAM column, which must stay fresh when a workspace
is renamed.

**TL;DR verdict:** The freeze is not caused by polling. It is caused by polling **over Apple
Events**, which macOS delivers synchronously to cmux's main thread — the same thread that draws
the UI and reads keystrokes. Every sweep makes cmux choose between answering fleetops and
answering the user.

Two better-looking transports were investigated and **both were empirically falsified** (below).
AppleScript stays. The fix is to make each sweep cheaper (bulk queries, not per-terminal property
access) and to stop running it every 2 s, because cmux topology is not 2-second-volatile data.

```
before:  ~175 ms of Apple Events every 2 s   ≈ 9 %   of cmux's main thread, permanently
after:    ~92 ms of Apple Events every 10 s  ≈ 0.9 %
```

## The measurement

All timings `osascript` on the operator's machine, 2026-07-19, 4 terminals, 5 runs averaged.
An empty script costs **35 ms** — that is `osascript` process startup, which does *not* touch
cmux. Subtracting it isolates the Apple Event cost that actually lands on cmux's main thread.

| variant | wall clock | **main-thread cost** |
|---|---|---|
| current (triple-nested, per-terminal `of tm`) | 210 ms | **175 ms** |
| per-tab bulk (`id of every terminal of t`) | 171 ms | 136 ms |
| **per-window bulk** (`… of every tab of w`) | 127 ms | **92 ms** |

All three emit **byte-identical output**, verified by `diff`, not by inspection.

The cost scales with terminal count, so the operator's real fleet pays more than this 4-terminal
sample. The task's own note measured ~60 ms per terminal.

## Candidates

### A. The cmux control socket — FALSIFIED

`cmux tree --all --json` returns everything `LIST_SCRIPT` returns *plus* `tty`, in **10 ms**, over
a Unix socket that never touches cmux's main thread. It also exposes `custom_color`, which
AppleScript does not.

**Why it fails:** the socket is gated by **process ancestry**, not by the
`CMUX_SOCKET_CAPABILITY` env var. A first test that merely stripped the cmux env vars appeared to
succeed — but that process was still a descendant of a cmux terminal, so the test proved nothing.
Re-run from a `launchd`-parented process with an empty environment:

```
$ launchctl submit -l t -- /bin/sh -c "env -i cmux tree --all"
Error: ERROR: Access denied — only processes started inside cmux can connect
```

`fleet` runs **outside** cmux. The socket is unreachable to it. This confirms spec 012's original
finding and the constraint documented at `src/cmux.rs:16-19` — that comment is accurate and stays.

Note the gate is on by default: `automation.socketControlMode` appears only inside the
commented-out defaults dump in `~/.config/cmux/cmux.json`, i.e. it is not explicitly configured,
and access is denied regardless.

### B. The cmux event bus — FALSIFIED

`cmux events` streams sequenced, replayable, resumable NDJSON (`--after`, `--cursor-file`,
`--reconnect`). It carries `workspace.renamed` and `workspace.action` with **resolved** values —
a rename to `fleets` and a color of `#C0392B` from the palette name `red`. A push design would
mean zero polling and instant freshness.

**Why it fails:** the bus is emitted by the **socket RPC layer**, not the model layer. Every
`workspace.renamed` frame observed carried a `method` / `params` / `result` envelope — the
signature of a CLI-originated call. Tested directly: the operator renamed a workspace via the
cmux **sidebar**, and across the resulting 201-event window **zero** rename events fired, while
`cmux workspace list` confirmed the title had in fact changed (`fleets` → `test`).

GUI renames are silent on the bus. Since the operator renames in the sidebar, a push design would
have worked in testing and failed in use — the worst possible failure mode.

(Reaching the bus at all also requires the socket, so candidate A's gate applies here regardless.)

### C. Cache the topology, refresh only when an unknown surface appears — REJECTED

Fetch once at startup; re-list only when a session reports a `surface_id` absent from the cache.
The trigger is exact, because sessions carry their own id from `ps`. Steady-state cost: zero.

**Why it fails:** it trades away rename freshness, which is the one property the operator
explicitly requires. A rename produces no new surface id, so it would never trigger a refresh.
Rejected by the maintainer on exactly this ground.

### D. Bulk queries + independent cadence — CHOSEN

Keep AppleScript. Make the sweep cheaper and rarer.

1. **Flatten `LIST_SCRIPT`** from a triple-nested loop doing per-terminal property access into
   per-window bulk queries. Each `id of tm` / `working directory of tm` / `name of t` inside the
   innermost loop is its own Apple Event round-trip; `id of every terminal of every tab of w`
   fetches the whole window in one. Measured 175 ms → 92 ms, byte-identical output.
2. **Decouple the cmux sweep from the 2 s sensor sweep.** Everything the operator watches at 2 s
   — status, tokens, ctx %, title — comes from `ps` and transcript tails, never from cmux. The
   cmux sweep supplies only window index, tab index, workspace name, and cwd, which change when a
   tab is created, closed, renamed, or reordered. 10 s is ample; `SurfaceCache` already serves
   the last-good list in between.

Renames keep working: `name of t` is still fetched every sweep, so a rename lands within one
cmux cadence.

## What this costs

Colors are unavailable, permanently, on this path. AppleScript exposes no color property on a
tab — probed under four spellings; `properties of tab 1 of window 1` returns exactly
`focused terminal, id, name, class, selected, index`. The only source of `custom_color` is the
socket, which candidate A rules out. **The maintainer dropped the colors requirement** on
2026-07-19 ("i dont need the colors at all for the fleet"), which is what makes candidate D
sufficient rather than merely best-available.

A rename now takes up to one cmux cadence (~10 s) to appear instead of up to 2 s. Accepted:
this is a monitoring board, and the columns in question are positional metadata.

## If the constraint ever changes

Running `fleet` from inside a cmux tab unlocks candidate A wholesale — 10 ms sweeps, no
main-thread contention, colors, `tty` for free, and the event bus for instant structural updates.
That is a launch-context decision, not a code decision. Everything needed to evaluate it is in
candidate A above.

## Cross-references

| Path | What |
|---|---|
| `specs/016-wave16-cmux-poll-cost.md` | The contract this plan feeds |
| `src/cmux.rs` | `LIST_SCRIPT`, `SurfaceCache`, the socket constraint comment |
| `src/tui/mod.rs` | `POLL` — the sweep cadence |
| `specs/012-wave12-cmux-lane.md` | Original socket finding, confirmed here |
