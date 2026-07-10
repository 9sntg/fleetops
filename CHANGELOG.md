# Changelog

## [Unreleased]

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
