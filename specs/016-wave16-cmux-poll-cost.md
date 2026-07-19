# Wave 16 — the cmux sweep stops freezing cmux

> Apple Events land on cmux's main thread. Ask less often, and ask for more per question.
>
> Status: **Draft** (agents never promote to Active — the maintainer does).
> Requested by the maintainer, 2026-07-18: "Launching fleetops makes cmux unresponsive."
> Decision dossier, including two falsified alternatives: `plans/003-cmux-poll-cost/`.

## Goal

Cut the share of cmux's main thread that fleetops consumes from ~9 % to ~1 %, with no change to
what the board displays.

## Constraints (verified 2026-07-19, do not re-derive)

- **AppleScript stays.** The cmux control socket is gated by process ancestry; a
  `launchd`-parented process is refused with `Access denied — only processes started inside cmux
  can connect`. `fleet` runs outside cmux. `src/cmux.rs:16-19` is accurate.
- **Colors are out of scope.** AppleScript exposes no color property on a tab; the maintainer
  dropped the requirement.
- **Renames must keep working.** The STREAM column reads `name of t` and must continue to.

## Data contract

### `LIST_SCRIPT` output is unchanged

One tab-separated row per terminal, exactly as today:

```
id \t window# \t tab# \t tabName \t cwd
```

The rewrite is a transport optimization and nothing else. Byte-identical output against the
current script is the acceptance test, not a nice-to-have.

`parse_list` is untouched. The fixture `tests/fixtures/cmux-terminals.txt` remains valid.

### Bulk queries replace per-terminal property access

The current script accesses `id of tm`, `working directory of tm`, and `name of t` inside the
innermost of three nested loops. Each accessor is an Apple Event round-trip, so cost scales with
terminal count.

Per window, the rewrite fetches three lists in three round-trips regardless of terminal count:

| query | yields |
|---|---|
| `name of every tab of w` | one name per tab |
| `id of every terminal of every tab of w` | one id list per tab |
| `working directory of every terminal of every tab of w` | one cwd list per tab |

Window and tab indices come from loop counters over those lists, preserving the 1-based
`window_index` / `tab_index` contract from spec 012.

The `character id 9` delimiter stays bound **outside** the `tell` block — cmux's `tab` class
shadows AppleScript's tab-character constant inside it (spec 012; module header).

### The cmux sweep gets its own cadence

| lane | source | cadence |
|---|---|---|
| sessions, telemetry, status, tokens, ctx % | `ps` + transcript tails | 2 s (unchanged) |
| cmux topology — window#, tab#, name, cwd | `osascript` | **10 s** |

`SurfaceCache` (`src/cmux.rs`) already folds a fetch result into `(surfaces, lane_error)` and
serves the last-good list on failure. Between cmux sweeps the board renders from that cache, so
rows keep their POS and STREAM values on the 2 s ticks that do not re-sweep cmux.

A rename appears within one cmux cadence.

## Behavior

- The board's rendered output is identical to today's, modulo when a topology change appears.
- Jump (`FOCUS_SCRIPT`, `cmux::focus`) is unchanged — it is keypress-driven and was never part of
  the cost.
- `fleet doctor` and `fleet snapshot` continue to call `cmux::list` directly; they are one-shot
  and unaffected by the board's cadence.
- A failed cmux sweep degrades exactly as today: stale list retained, footer reports the lane
  error.

## Done when

- `LIST_SCRIPT` output is byte-identical to the pre-change script against a live cmux.
- The cmux sweep runs on its own cadence, independent of the 2 s sensor sweep.
- Renaming a cmux workspace updates the STREAM column within one cmux cadence.
- cmux stays responsive with `fleet` running at normal workspace count.
- `./check.sh` green.

## Out of scope

- Workspace colors (unavailable via AppleScript; requirement dropped).
- The cmux control socket and event bus (both falsified — `plans/003-cmux-poll-cost/`).
- Any change to `parse_list`, `Surface`, `match_surface`, or the board's columns.
