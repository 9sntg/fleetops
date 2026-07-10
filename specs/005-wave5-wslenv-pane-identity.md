# Spec 005 — Wave 5: exact pane identity via WSLENV

> Dossier evolution path, trigger **fired 2026-07-10**: pane-match ambiguity observed live
> (`≈?` on two contoso sessions sharing cwd + title, and a initech pair). Fix = forward
> `WEZTERM_PANE` across the Windows→WSL boundary and use it as the highest-priority match.

## System change (one-time, outside the repo)

`WSLENV` user env var (HKCU, via `setx`) set to
`TERM:COLORTERM:TERM_PROGRAM:TERM_PROGRAM_VERSION:WEZTERM_PANE/u` — the four TERM vars
preserve today's effective forwarding; `WEZTERM_PANE/u` adds pane identity (Win32→WSL
direction). Takes effect for panes opened after the **next wezterm restart** (wezterm-gui
captured its env at startup). Bonus: the dormant `_cl_wezterm_account` guard in `.bashrc`
starts working.

## Behaviour

- discovery: read `WEZTERM_PANE` from `/proc/<pid>/environ` alongside `CLAUDE_ACCOUNT` →
  `LiveSession.wezterm_pane: Option<u64>`.
- Match priority (first hit wins):
  1. **exact** — env pane id present AND that pane exists in the wezterm list (guards dead
     panes / other windows) AND its id is unique across instances — pane ids are per-instance,
     so a cross-instance collision falls through to title/cwd; unresolved, it reads ambiguous
     (`≈?`), never a silent guess at the wrong window;
  2. title (ai-title or native name), unique or ambiguous as before;
  3. cwd fallback.
- Sessions started before the wezterm restart have no env pane id — graceful title/cwd fallback.
- doctor: new line `pane identity: N of M sessions exact (WSLENV WEZTERM_PANE)` (as shipped —
  fallback/unmatched detail already lives in the per-session ✓/✗ lines).

## Seams & tests

- `discovery`: environ parse extracts both vars in one pass; non-numeric WEZTERM_PANE → None.
- `board::match_pane`: exact beats title; env pane id not in the pane list → falls through to
  title; exact match never sets ambiguous.
- doctor report line with canned facts.
