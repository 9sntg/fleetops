# Wave 12/13 — the cmux lane: identity, topology, jump, highlight

> wezterm is gone. cmux exports `CMUX_SURFACE_ID` into every terminal it starts, so pane identity
> stops being inference and becomes an exact join — and the jump collapses to one command.
>
> Status: **Draft** (agents never promote to Active — the maintainer does).
> Waves 12 (lane) and 13 (jump/highlight) share one spec: they share one interface and one type
> refactor, and splitting them would land a `MatchedPane` that nothing can jump to.

## Goal

The PANE column and **Enter → jump** work on macOS, against cmux.

## Data contract (verified live 2026-07-16, this machine)

cmux's model is **Window → Workspace(tab) → Pane → Surface**. A surface is a terminal tab.

**Identity.** cmux exports these into each terminal's environment; `ps -AEwwo …,command=` reads
them back (spec 011's single call already carries them):

| Variable | Meaning |
|---|---|
| `CMUX_SURFACE_ID` | the surface this process runs in — **the join key** |
| `CMUX_WORKSPACE_ID` / `CMUX_TAB_ID` | its workspace/tab |
| `CMUX_CLAUDE_PID` | cmux's own record of the agent pid |
| `CMUX_SOCKET_CAPABILITY` | **an auth token — never captured** (allowlist, spec 011) |

**Topology + jump** come from cmux's shipped AppleScript dictionary
(`/Applications/cmux.app/Contents/Resources/cmux.sdef`):

```
application → windows → tabs → terminals
terminal:  id (== CMUX_SURFACE_ID), name, working directory
commands:  focus (…"bringing its window to the front"), select tab, activate window
```

Verified join, 2026-07-16 — every cmux-hosted session's `CMUX_SURFACE_ID` appears in the
AppleScript terminal list, and sessions in other terminals correctly have none:

```
pid=23028 -> 277E65DB-…  ->  window 1, tab 2
pid=23223 -> 02EC2459-…  ->  window 1, tab 1
pid=29328 -> FB1A26C1-…  ->  window 1, tab 4
pid=12696 -> (none: Apple Terminal)
```

Fixture: `tests/fixtures/cmux-terminals.txt` (captured live 2026-07-16, paths sanitized).

### Why AppleScript and not the cmux CLI

The `cmux` CLI is richer (`list-panes --json`, `focus-panel`, `trigger-flash`, `surface-health`)
but talks over a unix socket gated by `automation.socketControlMode`, whose default is
**`cmuxOnly`** — "only processes started inside cmux can connect". Using it would mean either
(a) `fleet` may only ever run inside a cmux pane, or (b) fleetops handles
`CMUX_SOCKET_PASSWORD`/`CMUX_SOCKET_CAPABILITY` — **a credential**, which CLAUDE.md forbids
fleetops from touching. AppleScript needs no credential, works inside and outside cmux, and is
an interface cmux ships deliberately. Cost: no `trigger-flash` (the highlight stays OSC 11), and
Automation permission is required (already granted on this box; a first run elsewhere prompts).

<!-- ponytail: if the CLI ever allows a read-only unauthenticated mode, revisit — `surface-health`
     and `trigger-flash` are strictly better than what AppleScript exposes. -->

## Behaviour

1. **Identity is exact or absent.** `CMUX_SURFACE_ID` names exactly one surface (ids are UUIDs).
   No surface id → not under cmux. Id absent from the list → the surface closed since the sweep.
   Both are plain "no match". **The wezterm-era title/cwd tie-break tiers and the `ambiguous`
   outcome are deleted** — with identity there is nothing left to guess, so pre-mortem #4 is
   satisfied by construction rather than by flagging (`≈?` in the PANE column is retired too).
2. **PANE column** shows the 1-based cmux tab number, or `—`.
3. **Jump** = `focus <terminal>`: one call that raises the window and focuses the surface. The
   ordered `activate-tab` → `activate-pane` hazard is structurally gone.
4. **Injection-proof.** The surface id is passed as `osascript` **argv** (`on run argv`), never
   interpolated into the script. Verified live: a `" & (do shell script "…") & "` payload returns
   `notfound` rather than executing.
5. **Degradation.** cmux not running / Automation denied → `⚠ cmux unreachable` in doctor, the
   footer says the lane is degraded, the board still renders every session. A transient failure
   keeps the last-good topology (`SurfaceCache`) — stale beats blank.
6. **A session outside cmux is normal**, not an error: Enter reports
   `jump: '<name>' isn't in a cmux surface — not started under cmux?`.
7. **Highlight (wave 13)** stays OSC 11 to the session's tty, now `/dev/ttysNNN` from spec 011's
   `ps`. The fcntl flags are corrected to Darwin's values — see below.

## The wave-13 bug this fixes

`highlight.rs` hardcoded **Linux** fcntl values to avoid a `libc` dep:

| Const | Was (Linux) | On Darwin that is | Correct Darwin value |
|---|---|---|---|
| `O_NOCTTY` | `0o400` | `O_NOFOLLOW` | `0x00020000` |
| `O_NONBLOCK` | `0o4000` | `O_EXCL` | `0x00000004` |

`O_EXCL` without `O_CREAT` is ignored, so the open would have **succeeded without non-blocking
semantics** — a write to a wedged pty could block. It would never have failed loudly, because
`write_bytes` drops every error by design. Ground truth: Darwin `sys/fcntl.h`. Pinned by test.

## Seams & structure

New `src/cmux.rs`, mirroring the proven four-layer split:

| Layer | Item |
|---|---|
| pure script/spec builders | `LIST_SCRIPT`, `FOCUS_SCRIPT`, `FOCUSED_SCRIPT`, `list_spec`, `focus_spec`, `focused_spec` |
| pure parser | `parse_list(bytes) -> Vec<Surface>` |
| thin async fetch | `list`, `focus`, `focused_surface_id` |
| last-good cache | `SurfaceCache::fold` |

`board::match_pane(env_pane, cwd, names, panes) -> (Option<MatchedPane>, bool)` becomes
`board::match_surface(surface_id, surfaces) -> Option<MatchedPane>`.
`MatchedPane { socket, tab_id, pane_id, tab_index }` becomes `{ surface_id, window_index, tab_index }`.

**Deleted:** `src/panes.rs` (1047 lines) and `tests/fixtures/wezterm-list.json` entirely —
`tasklist.exe`, `wezterm.exe`, `wsl_to_win`/`win_to_wsl`, `WSLENV` forwarding, the
`/mnt/c/Users/*` socket-glob ladder, `SockDir`, multi-instance partial-failure merging,
`classify_title`, `SessionRow::pane_ambiguous`, and `AppError::Parse` (the tolerant line-oriented
parsers cannot construct it).

### An AppleScript trap, encoded as a constraint

cmux's dictionary defines a `tab` **class**, which shadows AppleScript's `tab` (tab-character)
constant inside a `tell` block — the delimiter silently emits as the literal text `tab`, and the
whole parse yields garbage. `LIST_SCRIPT` therefore binds `set d to character id 9` **outside**
the `tell`. Pinned by test.

## Deterministic tests (red first)

- 🔴 `cmux::tests` — fixture → surfaces; ids unique (so the match can be exact); a cwd with
  spaces survives (tab-separated, not space-separated); malformed rows skipped; garbage → empty;
  `focus_spec` carries the id as argv[2] and never in the script body; the delimiter binds
  outside `tell`; `SurfaceCache` keeps last-good on failure; `list`/`focused_surface_id`/`focus`
  via `CannedRunner` (no spawn); `focus` sends exactly ONE call.
- 🔴 `procsrc::tests` — the allowlist drops `CMUX_SOCKET_CAPABILITY` and argv (security pin).
- 🔴 `board::tests` — `match_surface` table; a shared cwd cannot cause a mismatch.
- 🔴 `highlight::tests` — the Darwin fcntl values, with the Linux ones asserted absent.
- ♻ `./check.sh` green (132 tests).

## Out of scope

- The Codex lane still carries no surface id (`codex.rs` sets `surface_id: None`) — wave 14.
- `trigger-flash`, `surface-health`, multi-window PANE display (`w:t`).

## Dependencies

**None added.** `osascript` + `ps` via the existing `Runner` seam.
