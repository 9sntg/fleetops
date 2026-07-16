# Wave 15 — STREAM + BRANCH columns

> Which cmux workstream is this session in, and what branch is it on? Both are one lookup away.
>
> Status: **Draft** (agents never promote to Active — the maintainer does).
> Requested by the maintainer, 2026-07-16: "show cmux workstream name (if exists) and branch,
> between dir and session."

## Goal

Two new columns between DIR and SESSION: **STREAM** (the cmux workspace name) and **BRANCH**.

## Data contract (verified live 2026-07-16)

### STREAM — the cmux workspace name

cmux calls a window's tabs *workspaces*, and its Feed calls a unit of work a *workstream*.
AppleScript's `tab` class exposes `name` — the workspace title shown in cmux's tab bar:

```
tab 1  gtm-studio            /Users/user/Desktop/groupon-gtm-studio
tab 2  email-signal-capture  /Users/user/Desktop/gtm-worktrees/email-signal-capture
tab 6  skills                /Users/user/Desktop
tab 7  rules                 /Users/user/Desktop
```

It rides in on the existing `LIST_SCRIPT` — no new call.

**A tab name may carry a leading status glyph.** cmux auto-names a workspace from its agent's
title until the user renames it, and that auto-name includes the agent's glyph (observed live:
`✳ Check current global skills`, `⠐ Review project rules and guidelines`). The glyph is the
agent's status, which the STATUS column already shows, so it is stripped — the same braille
(`U+2800‑28FF`) / `✳` convention wave 12 retired `classify_title` for.

Note tabs 6 and 7 share a cwd (`/Users/user/Desktop`) with different names — STREAM is NOT
derivable from cwd, which is exactly why it comes from the surface match (spec 012's exact id).

### BRANCH — pure filesystem, no `git` subprocess

Three shapes, all verified on the maintainer's real session cwds:

| cwd | `.git` | resolution |
|---|---|---|
| `groupon-gtm-studio` | **dir** | `<cwd>/.git/HEAD` → `ref: refs/heads/main` → `main` |
| `gtm-worktrees/email-signal-capture` | **file** | `gitdir: …/groupon-gtm-studio/.git/worktrees/email-signal-capture` → that dir's `HEAD` → `feat/email-signal-capture` |
| `Desktop`, `FAI` | absent | `None` |

Worktrees are not an edge case here — 4 of 9 live sessions run in one, so the `gitdir:`
indirection is the common path, not a nicety.

Also handled: a cwd **below** the repo root (walk up to the first `.git`), and a **detached
HEAD** (`HEAD` holds a raw sha, not `ref: `) → short sha.

<!-- ponytail: reading .git/HEAD is a filesystem read per session per sweep (~4 for the
     maintainer). If a big fleet ever makes that measurable, cache on (cwd, HEAD mtime) — but
     do not reach for `git` subprocesses: that would be one spawn per session per sweep. -->

## Behaviour

1. **STREAM** = the matched surface's tab name, glyph-stripped. `—` when the session isn't in a
   cmux surface (started elsewhere) — same rule as PANE.
2. **BRANCH** = the branch for the session's cwd, or `—` when the cwd isn't in a git repo.
   Detached HEAD renders the short sha.
3. Column order: `# | STATUS | DIR | STREAM | BRANCH | SESSION | CTX | TOK | ACCT | AGE | PANE`.
4. Both apply to **Codex rows too** — same cwd, same surface match.
5. Neither can fail a sweep: an unreadable `.git` is `None`, never an error.

## Seams & structure

- `cmux::Surface` gains `name: String`; `LIST_SCRIPT` emits it (no new call). `strip_status_glyph`
  is pure and table-tested.
- New `src/git.rs` — `branch_of(cwd: &Path) -> Option<String>`, plus the pure parsers
  `branch_from_head(&str)` and `gitdir_from_file(&str)`. Only `branch_of` touches the fs.
- `board::SessionRow` gains `stream: Option<String>` + `branch: Option<String>`;
  `board::MatchedPane` gains `stream: String`.
- **`board::assemble` stays pure.** Branches are resolved in `collect` (which already does the
  blocking fs work) and passed as a parallel slice, exactly like `telemetry` — the sensor hands
  assembly data already in hand. `codex::scan` resolves its own, since it already touches the fs.

## Deterministic tests (red first)

- 🔴 `git::tests` — `branch_from_head` (`ref: refs/heads/feat/x` → `feat/x`; a raw sha → short
  sha; garbage → None); `gitdir_from_file`; a tempdir **worktree** (`.git` FILE →
  `worktrees/<n>/HEAD`) resolves; a cwd below the root walks up; a non-repo → None; an
  unreadable `.git` → None, never a panic.
- 🔴 `cmux::tests` — the fixture parses names; `strip_status_glyph` table (braille, `✳`, a name
  with no glyph, a name that legitimately starts with a letter).
- 🔴 `board::tests` — stream/branch reach the row; absent surface → stream `None`.
- 🔴 `view::tests` — header order; both columns render; `—` placeholders.
- 🟢 `./check.sh` green.

## Out of scope

- Dirty/ahead-behind indicators. Branch name only.
- Watching `.git` for changes (the 2 s sweep re-reads it).

## Dependencies

**None added.** `std::fs` + the existing AppleScript call.
