# Wave 11 — macOS process source

> The board is empty on macOS because liveness is gated on `/proc`. Replace the process source
> with `ps`, behind the existing `Runner` seam, and the fleet appears.
>
> Status: **Draft** (agents never promote to Active — the maintainer does).
> Requested by the maintainer, 2026-07-16: "convert this project to a mac project so i can see
> my claude sessions." fleetops targets **macOS only** from this wave on; the WSL2/Linux
> `/proc` source is deleted, not cfg-gated.

## Goal

`fleet` on macOS lists every live Claude Code session, with the PID-reuse guard intact.

## Data contract (verified live 2026-07-16, this machine)

`~/.claude/sessions/<pid>.json` is **byte-identical in shape** to the Linux fixture — same
required fields (`pid`, `sessionId`, `procStart`), same optionals. One field's *content* differs:

| Field | Linux | macOS |
|---|---|---|
| `procStart` | `/proc/<pid>/stat` field 22, clock ticks since boot — e.g. `"126796"` | wall-clock **UTC**, `ctime`-style — e.g. `"Thu Jul 16 19:10:07 2026"` |

fleetops never *interprets* `procStart` — it string-compares it (A1). So the liveness invariant
survives unchanged; only its source moves:

```
TZ=UTC ps -Ao pid=,tty=,lstart=
  12696 ttys000 Thu Jul 16 19:10:07 2026
```

`TZ=UTC` is **load-bearing**: `procStart` is UTC, `ps` formats `lstart` in local time (this box
is CDT — a naive `ps` is 5h off and *every* session reads as PID-reused).

<!-- ponytail: lstart day-of-month is space-padded by ps ("Jul  6" vs "Jul 16"). Whether Claude
     Code pads identically is unverified — only a 2-digit day was observable on 2026-07-16.
     Both sides are whitespace-normalized before comparison so padding can never decide liveness. -->

Fixture: `tests/fixtures/ps-table.txt` (captured live 2026-07-16).

## Behaviour

1. **Liveness** — a session is live iff `ps` reports its pid AND that pid's normalized `lstart`
   equals the file's normalized `procStart`. Same PID-reuse guard, same string equality.
2. **tty** — `ps` field `tty=` (`ttys000`) becomes `/dev/ttys000`. `??` (no controlling
   terminal) → `None`. Replaces the `/proc/<pid>/fd/1` → `/dev/pts/` symlink read.
3. **A failed `ps` is not an empty fleet.** If the process table can't be fetched, sessions are
   NOT silently scored stale-dead — `ScanStats::procs_unavailable` is set, the footer and doctor
   say so, and doctor exits non-zero. This closes the Linux-era hole where a missing `/proc` and
   a genuinely dead PID shared one code path (the board read as "nothing running").
4. **Account attribution is deferred to wave 12.** `CLAUDE_ACCOUNT` lived in
   `/proc/<pid>/environ`; the macOS equivalent (`ps -Eww`) is only worth one call once the pane
   lane needs `CMUX_SURFACE_ID` from the same output. `account` is `None` this wave.
   (`CLAUDE_ACCOUNT` is unset on the maintainer's machine — the ACCT column is empty either way.)
5. **`fleet doctor`** drops `⚠ /proc not found — fleetops targets WSL2/Linux` and gains
   `⚠ process table unavailable: {e} — every session reads as dead` on a `ps` failure.

## Seams & structure

New `src/procsrc.rs` — the process source, mirroring `panes.rs`'s proven four-layer split so the
whole lane is testable with no process spawn:

| Layer | Item |
|---|---|
| pure argv/spec builder | `table_spec()` — `ps -Ao pid=,tty=,lstart=` + `TZ=UTC` env override |
| pure parser | `parse_table(bytes) -> ProcTable` |
| pure normalizer | `normalize_lstart(&str) -> String` |
| thin async fetch | `fetch(runner: &dyn Runner) -> AppResult<ProcTable>` |

`ProcTable = HashMap<u32, ProcFacts>`; `ProcFacts { lstart: String, tty: Option<String> }`.

`discovery::scan(sessions_dir, procs: &ProcTable)` replaces `scan(sessions_dir, proc_root: &Path)`.
The table is **fetched async and passed into the blocking scan as plain data** — exactly the
pattern `collect::collect` already uses for `panes_result`, and the reason `scan` stays sync.

Deleted: `starttime_from_stat`, the `/proc/<pid>/{stat,environ,fd/1}` reads in `scan`, the
`fake_proc` test builder, and the three `Path::new("/proc")` sites (`collect.rs:50`,
`doctor.rs:156`/`:164`). `discovery::parse_environ` and `starttime_from_stat` survive **only**
because `codex.rs` still calls them; wave 14 deletes them with the Codex `/proc` walk.

## Deterministic tests (red first)

- 🔴 `procsrc::tests` — `parse_table` over the live fixture; a `??` tty → `None`; a malformed
  line is skipped not fatal; garbage → empty table; `normalize_lstart` collapses the padded-day
  form (`"Thu Jul  6 …"` == `"Thu Jul 6 …"`); `table_spec` carries `TZ=UTC` and never a shell string.
- 🔴 `discovery::tests` — `scan` over a tempdir + a canned `ProcTable`: live kept, dead dropped,
  **PID-reused dropped** (same pid, different lstart), parse failures counted, `/dev/ttys` built
  from the table, `??` → `None`.
- 🔴 `doctor::tests` — `procs_unavailable` prints the warning; the clean report has no `⚠`.
- 🟢 wire `collect`/`sweep`/`snapshot`/`doctor` to fetch the table alongside the pane list.
- ♻ refactor-for-specs, ♻ refactor-for-rules; `./check.sh` green.

## Out of scope

- The pane lane (still wezterm, still degraded on macOS — wave 12 replaces it with cmux).
- The Codex lane (still walks `/proc`, returns empty on macOS — wave 14).
- Account attribution (wave 12, folded into the one `ps -Eww` the identity lane needs).

## Dependencies

**None added.** `ps` via the existing `Runner` seam + `std::collections::HashMap`. No `libproc`,
no `sysinfo`, no `nix` — the "ask first: new external dependency" gate is not triggered.
