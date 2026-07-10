# Changelog

## [Unreleased]

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
  drawn by the lua tab formatter). CWD column → DIR: last folder only (`brain`, `contoso`).
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
