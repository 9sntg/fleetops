# Contributing to fleetops

Thanks for your interest. fleetops is passively maintained — issues and PRs are welcome, but
reviews may be slow.

## The gate

One thing is non-negotiable: **`./check.sh` must be green** before a change is done. It runs:

```bash
./check.sh   # cargo fmt --check + clippy (all/pedantic/nursery/cargo, -D warnings) + rustdoc + cargo test
```

Lints are strict (`unsafe_code` is forbidden crate-wide; clippy pedantic/nursery/cargo deny
warnings). If a new lint allow is genuinely warranted, justify it inline next to the `allow`.

## Working style

- This is a **macOS + cmux** tool. Session discovery shells out to `ps`; the pane/jump lane uses
  cmux's AppleScript interface via `osascript`. Both go through the `Runner` seam, so keep core
  logic pure and testable off the OS/network (see the `#[cfg(test)]` tables throughout `src/`).
- Development is spec-driven: specs live in `specs/` (index: `specs/README.md`), coding rules in
  `rules/` (start at `rules/_index.md`). Every source file carries a `//!` module header
  (`rules/file-headers.md`).
- Add a `CHANGELOG.md` `[Unreleased]` entry for every user-facing change, in the same commit.
- Never log or print an access/refresh token; fleetops reads no credentials and must stay that way.

## License of contributions

By submitting a contribution you agree it is dual-licensed under **MIT OR Apache-2.0**, matching the
project, without any additional terms or conditions.
