# Changelog

## [Unreleased]

- **The cmux lane replaces wezterm — pane identity is now exact (spec 012, waves 12–13)** — cmux
  exports `CMUX_SURFACE_ID` into every terminal it starts, and spec 011's single `ps` call already
  reads process environments, so a session's surface comes free with its liveness. Topology and
  the jump use cmux's shipped **AppleScript** interface (`cmux.sdef`), not its control socket:
  the socket's default `automation.socketControlMode = cmuxOnly` would force `fleet` to run
  inside cmux or to handle `CMUX_SOCKET_CAPABILITY` — a credential fleetops must never touch.
  AppleScript needs none and works inside or outside cmux.
  - **Matching is exact or absent.** A surface id names exactly one surface (UUIDs), so the
    title/cwd tie-break tiers, the `ambiguous` outcome and the `≈?` PANE marker are **deleted** —
    there is nothing left to guess. A session started outside cmux simply shows `—`.
  - **Jump is one call.** `focus <terminal>` raises the window AND focuses the surface, so
    wezterm's ordered `activate-tab` → `activate-pane` hazard is structurally gone.
  - **Injection-proof:** the surface id rides as `osascript` argv, never interpolated into the
    script. Verified live — a `do shell script` payload returns `notfound` rather than executing.
  - Removed: `src/panes.rs` (1047 lines), `tests/fixtures/wezterm-list.json`, `tasklist.exe`,
    `wezterm.exe`, `WSLENV` forwarding, the `/mnt/c/Users/*` socket-glob ladder, multi-instance
    partial-failure merging, and `AppError::Parse`.
  - `fleet snapshot` contract change: `focused_pane_id` → `focused_surface_id` (string),
    `pane_id` → `surface_id` (string); `tab_index` is now the 1-based cmux tab.

- **Fixed: the pane-highlight used Linux fcntl flags on macOS (wave 13)** — `O_NOCTTY`/`O_NONBLOCK`
  were hardcoded to Linux's asm-generic values (`0o400`/`0o4000`), which on Darwin mean
  `O_NOFOLLOW` and `O_EXCL`. Since `O_EXCL` without `O_CREAT` is ignored, the pty opened
  **without non-blocking semantics** — a write to a wedged pty could block — and it could never
  fail loudly because the writer drops every error by design. Now Darwin's `0x20000`/`0x4`, pinned
  by a test and re-verified against a real pty.

- **macOS process source — the board works on macOS (spec 011, wave 11)** — session liveness no
  longer needs Linux `/proc`, so the fleet is visible on a Mac. macOS Claude Code records
  `procStart` as a UTC wall-clock string (`"Thu Jul 16 19:10:07 2026"`) rather than Linux clock
  ticks, and `TZ=UTC ps -Ao pid=,tty=,lstart=` reproduces it byte-for-byte — so the PID-reuse
  guard is unchanged (still a string equality, still opaque), only its source moved. `TZ=UTC` is
  load-bearing: `procStart` is UTC and `ps` formats in local time, so without it every session
  reads as PID-reused. Both sides are whitespace-normalized, so ps's space-padded day-of-month
  ("Jul  6") can never decide liveness. The session's tty (`/dev/ttys000`) now comes from the same
  single `ps` call that establishes liveness, replacing the `/proc/<pid>/fd/1` symlink read.
  fleetops is **macOS-only** from this wave: the `/proc` source is deleted, not cfg-gated.

- **Fixed: a broken process source no longer reads as an empty fleet** — on Linux a missing
  `/proc` and a genuinely dead PID shared one silent code path, so the board rendered a clean,
  empty, *wrong* fleet and `fleet doctor` still exited 0. A failed `ps` now sets
  `ScanStats::procs_unavailable`, prints `⚠ process table unavailable: {e} — every session reads
  as dead, this is NOT an empty fleet`, and exits non-zero.

- **Fixed: wezterm socket dir is resolved at runtime, not a hardcoded username** — the
  release-prep scrub replaced the author's Windows username in the socket-dir constants with a
  placeholder (`user`), which pointed pane discovery at `/mnt/c/Users/user/.local/share/wezterm`
  — a path that exists on no real machine, so the board showed zero panes for everyone. The
  username is now resolved per process: `$WEZTERM_UNIX_SOCKET` (wezterm's own var) if set, else a
  glob of `/mnt/c/Users/*/.local/share/wezterm` for the dir holding live `gui-sock-*` files
  (preferring the `$USER`-named dir, else the newest socket). On no match it degrades exactly as
  before — no per-instance discovery, the board still renders the invoker's own instance.

- **Public-release prep** — repo readied for an open-source release: dual-licensed
  (MIT OR Apache-2.0) with `LICENSE-MIT` + `LICENSE-APACHE`; Cargo metadata filled in
  (license, readme, keywords, categories, rust-version); a stranger-facing `README.md`
  (features, WSL2-only platform note, a "What it reads & privacy" section, unofficial
  disclaimer); `CONTRIBUTING.md` + `SECURITY.md`; GitHub CI (`fmt`/`clippy`/`test`),
  Dependabot, and issue templates; a VHS demo recipe (`docs/demo/board.tape`). Fixtures,
  tests, specs, plans, and rules were scrubbed of machine- and account-specific data
  (synthetic account/dir/session names; the seed constants behind `account_color`/`dir_badge`
  re-tuned to the new sample sets). `fleet doctor` now prints a WSL2/Linux hint when `/proc`
  is absent. `docs/ops/` is no longer tracked.

- **Wave 10 (spec 010): board `#` agent-number column + snapshot `age_secs`** — the board's
  leading column is now **`#`**, showing the **agent board number `n`** (1-based row order — the
  same `n` `fleet snapshot` emits), replacing the TAB column (its `tab_index + 1` display and the
  `tab_cell` placeholder are gone). New order: `# | STATUS | DIR | SESSION | CTX | TOK | ACCT |
  AGE | PANE` — the **PANE column stays** (the jump target a human reads is the pane id). `n` shows
  on every row, including sessions with no matched pane (no placeholder — it is pure order). The
  snapshot JSON is otherwise unchanged: it still carries the 0-based `tab_index` for automation
  (`wezterm cli activate-tab --tab-index`); the board just stops *displaying* the tab number.
  - `fleet snapshot` `sessions[]` gains **`age_secs`** (`number|null`) — seconds since the
    transcript last appended (`SessionRow.secs_since_append`, the raw age the AGE column
    humanizes), `null` when unknown. Placed after `ctx_pct`; every other field (incl. `tab_index`)
    unchanged. This lets downstream surfaces (the Stream Deck FLEET badges) show the age without a
    second telemetry path.

- **Wave 9 (spec 009): `fleet snapshot` + a leading TAB column** — a headless one-shot,
  `fleet snapshot`, prints ONE JSON object to stdout: `focused_pane_id` (from
  `wezterm cli list-clients` — the least-idle client's focused pane) plus a `sessions` array
  carrying exactly the rows the board would render, in the same order (`n`, `name`, `status`
  = the exact `fold::Status` variant name, `tokens`, `ctx_pct`, `pane_id`, `tab_index`, `cwd`,
  `session_id`). It reuses the board's own pipeline: the sensor sweep is extracted into
  `collect::collect`, called by BOTH `tui::sweep` and `snapshot::run`, so a snapshot and the
  live board can never disagree. Exit 0 on success (even with 0 sessions), non-zero only on scan
  failure (sessions dir unreadable, same rule as `fleet doctor`). Serialized with `serde_json`
  (already a dep); no new dependency.
  - The JSON **`tab_index` is 0-based**, matching `wezterm cli activate-tab --tab-index` (0 =
    left-most tab) so automation can drive `activate-tab` with it directly. The board's TAB
    **column displays `tab_index + 1`** — 1-based, matching the wezterm tab bar
    (`format-tab-title` renders `tab.tab_index + 1`) and the Stream Deck TAB keys (1–6). The
    column moves to **before STATUS** (`TAB | STATUS | DIR | SESSION | CTX | TOK | ACCT | AGE |
    PANE`), one column not two, with a dim `—`/`≈?` placeholder when unmatched.

- **Wave 8 (spec 008): Codex CLI sessions on the board** — the fleet now also shows Codex TUI
  sessions (`~/.codex`) alongside Claude ones: same columns, same sort buckets, same Enter-jump
  and pane highlighting. Discovery recognizes a live Codex TUI by `/proc` facts (`comm ==
  "codex"`, argv0-only cmdline, `fd/1 -> /dev/pts/*` — the node shim and `codex exec`/
  `--version` are skipped for free) and joins it to its newest same-cwd rollout
  (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`) without a sqlite dependency: a liveness guard
  rejects a rollout older than the process's own start minus a 600 s slack, and two live
  processes sharing a cwd never join (never guess which rollout is whose — `codex — session
  ambiguous`). SESSION name is the last `user_message`, truncated to 60 chars (`codex — no
  prompt yet` pre-first-prompt). CTX/TOK come from the rollout's own `token_count` line against
  its own `model_context_window` — never Claude's 200k/1M inference. Footer appends `· N codex`
  when N > 0 (Codex rows aren't tallied in the Claude-only live count). `SessionRow` gains a
  `ctx_pct` field as the ctx% seam between the two lanes; `board::sort_rows` is now a standalone
  function so the sweep can concatenate Claude + Codex rows and sort once.
  - **Review fixes**: quit always resets pane tints, even when the event stream dies (cleanup no
    longer bypassed by an early return); Codex rollout cap now sorts by mtime (a long-running
    session's rollout can no longer age out of the cap by filename); rollout tail window raised
    64 KiB → 256 KiB to match telemetry's; a joined rollout with no prompt yet in its tail reads
    "codex (untitled)", not the misleading "no prompt yet"; the Codex ctx% bar never falls back
    through Claude's 200k/1M inference (renders "—" instead); a pane reused by a new session
    within one sweep no longer races the old session's tint reset against the new one's write;
    Codex session names take only the prompt's first line before truncating to 60 chars.
- **Wave 7 (spec 007): DIR up front, with a project badge** — column order is now
  `STATUS | DIR | SESSION | CTX | TOK | ACCT | AGE | TAB | PANE` (DIR moved next to STATUS,
  ahead of SESSION). The DIR cell renders `<emoji> <dir_name>`, colored — a pure hash
  (`dir_badge`, same djb2 + splitmix64 recipe as `account_color`, independent seeds for the
  emoji pick and the color pick) so a project keeps the same emoji + color across sessions,
  accounts, and restarts; no config file or per-project mapping.

- **Wave 6 (spec 006): pane highlighting** — sessions needing attention tint their own wezterm
  pane background via OSC 11, written straight to the session's `/dev/pts/N` (no `.wezterm.lua`
  changes): steady amber for NeedsAnswer/Waiting, steady dark red for Stalled. A Working → Idle
  transition ("just finished") pulses bright/dim green (~1 s) and settles into a sticky dark
  green that stays until "noticed" (status leaves Idle, or Enter-jump to that row). First-sweep
  hygiene resets any stale tint left by a crashed/killed previous `fleet`; quit resets every
  currently-tinted pane (bounded, never hangs exit). Opt out with `fleet --no-highlight` (the
  model still computes tints, the loop just drops the writes).

- **Strictest-lints pass + 22-finding review sweep** (code-reviewer + code-simplifier plugins,
  4 review dimensions, adversarial verification; all confirmed findings applied TDD-style):
  - Lint gate hardened: clippy `nursery` + `cargo` groups and `missing_docs` now deny; rustdoc
    `-D warnings` added to `check.sh`; crossterm pinned to ratatui's 0.28 (one copy links);
    `cargo audit` clean (2 transitive warnings via ratatui, code paths unused).
  - **Exact pane match instance-aware**: `WEZTERM_PANE` ids are only unique per wezterm
    instance — a cross-instance id collision now falls through to title/cwd (unresolved → `≈?`)
    instead of silently jumping to the wrong window's pane.
  - **Partial instance failure surfaces**: one wezterm instance failing no longer returns a
    silent partial pane list — the footer/doctor report the degraded instance, and the partial
    list never overwrites the last-good pane cache (stale-beats-blank now holds for the exact
    failure class it was built for).
  - `--no-auto-start` on every wezterm cli call (a socketless sweep must error, not spawn a mux
    server); drvfs socket stats moved off async workers; manual-refresh autorepeat coalesced;
    transient transcript read failures are no longer cached as "empty transcript" (could hide a
    pending question forever); terminal restore failure and event-stream death no longer exit
    silently with success; footer counts unparseable session files; ACCT column fits 8-char
    accounts; doctor: scan-task crash exits 1, spec-005 pane-identity line pinned by test.
  - New tests: Exec timeout bound (the anti-freeze invariant), zero-usage clobber-guard,
    environ-less live session, cache heal-after-failed-read, merge/fold partial-failure tables.

- **Multi-instance wezterm sensor** — root cause of the empty TAB/PANE board: two wezterm-gui
  processes run (main + TUI monitor), and a `cli` call only answers from the instance owning
  the invoking pane, so fleet on the monitor saw zero Claude panes. Fleet now discovers every
  live instance (tasklist PIDs × gui-sock files; stale sockets HANG and are filtered) and
  queries each via a WSLENV-forwarded `WEZTERM_UNIX_SOCKET` (`/w`). Jumps target the pane's
  own instance.
- Board visuals: CTX column is a colored 10-cell gauge (green/yellow/red, no percentage);
  AGE is color-coded (fresh green, minutes yellow, stale red, ancient dimmed).

- Board UX: Enter now switches the TAB too (`activate-tab` before `activate-pane` — pane
  activation alone doesn't bring the tab forward). New TAB column right after STATUS showing
  the tab-bar position number (tab emoticons aren't readable via the wezterm CLI — they're
  drawn by the lua tab formatter). CWD column → DIR: last folder only (`project-a`, `project-b`).
- Wave 5 (spec 005): exact pane identity — `WEZTERM_PANE` now crosses into WSL (WSLENV user
  env var set Windows-side; effective for panes opened after a wezterm restart). Match
  priority: exact env pane > title > cwd; doctor reports the exact-identity count. Installed
  `fleet` on PATH (`~/.local/bin/fleet` → release binary, same pattern as `gc`).
- Waves 2–4 (specs 002–004): rows are now **sessions**, not panes. Discovery from
  `~/.claude/sessions/*.json` with /proc liveness (procStart match) and `CLAUDE_ACCOUNT`
  attribution; transcript-tail telemetry (ctx%, tokens, ai-title, pending AskUserQuestion) with
  a (size,mtime) cache; pure status fold — NeedsAnswer / **Waiting** (new native status found
  live, disproves dossier A6) / Stalled? / Unknown / Working / Idle / Shell — with 300 s stall
  detector; title-first pane matching (WSL pane cwds are Windows paths — corrected in
  RESEARCH.md); account colors; `fleet doctor` read-only drift report.
- Wave 1 (spec 001): the `fleet` board — polls `wezterm cli list` every 2 s, shows Claude panes
  (working/idle glyph, semantic name, cwd, tab:pane), j/k selection survives refresh, Enter jumps
  to the pane, r forces refresh, footer shows staleness + sensor errors.
- Repo scaffold: rules port (from tokenomics/vault), toolchain pins, check gate, data-source recon.
