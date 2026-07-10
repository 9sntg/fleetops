# fleetops

Starfleet fleet operations board for Claude Code: one TUI monitoring every running session —
semantic name, status (working / done / needs input), tokens spent, context %, and the wezterm
pane it lives in. Each session is a ship; fleetops is the ops board.

Part of the `/tui` fleet: `tokenomics` (accounts/limits) · `ground-control` · `ghmonitor` · `bridge`.

- Architecture dossier: `plans/001-*` · Data-source recon: `docs/RESEARCH.md`
- Rules: `rules/_index.md` · Gate: `./check.sh`
