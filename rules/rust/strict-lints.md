---
rule: rust/strict-lints
title: Strict Rust Lints & Toolchain
category: rust
scope: [rust]
priority: required
applies-to: [rust]
tags: [clippy, rustfmt, lints, unsafe, ci, toolchain]
---

# Strict Rust Lints & Toolchain

**Enforcement**: the `check` script (`cargo fmt --check && cargo clippy -- -D warnings && cargo test`) gates every wave. CI-equivalent locally; nothing merges red.

---

## Core Principle

> The compiler and clippy are the cheapest reviewers you have. Turn them all the way up and never argue with a green build.

Warnings are errors. Unsafe is forbidden. Formatting is not a preference. These are set once in config so no one re-litigates them per file.

---

## Toolchain is pinned

`rust-toolchain.toml` at the repo root pins the channel and required components so every machine and agent builds identically:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
profile = "default"
```

Do not rely on whatever `stable` a given box happens to have — the pin is the contract.

## Lints live in `Cargo.toml`, not scattered `#![deny]` attributes

Use the `[lints]` table so the policy is declared once and inherited by every target (lib, bins, tests):

```toml
[lints.rust]
unsafe_code = "forbid"          # this crate shells out; it never needs unsafe
unreachable_pub = "warn"
missing_debug_implementations = "warn"
rust_2018_idioms = { level = "warn", priority = -1 }

[lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "deny", priority = -1 }
# Curated, justified allows only — each with a reason comment:
module_name_repetitions = "allow"   # supervisor::SupervisorError reads fine
missing_errors_doc = "allow"        # error types are self-describing here
```

- `forbid(unsafe_code)` is non-negotiable for this crate. Signal handling and pgid kills go through the `nix` **safe** wrappers — if you reach for `unsafe`, you have taken a wrong turn.
- Every `allow` carries a one-line reason. An un-commented `allow` fails review.
- Prefer fixing the lint over allowing it. `#[allow(...)]` at a call site (not crate-wide) is acceptable for a single justified exception, again with a reason.

## Formatting

`rustfmt.toml` sets the house style; `cargo fmt --check` must pass. Do not hand-format around rustfmt.

## The `check` gate

A `check` script/recipe is the single command that must be green before a wave is "done":

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Run it after every refactor step. "It compiles" is not the bar — `-D warnings` clean is.

## Hard rules

- **No `warnings` left behind.** Not "I'll clean them later." The gate is red until they're gone.
- **No `unwrap()` / `expect()` / `panic!` in runtime paths** (see [[error-handling]]). Tests may `unwrap`.
- **No `unsafe`.** Forbidden crate-wide.
- **No `#[allow]` without a reason comment.**
- **`dbg!`, `todo!`, `unimplemented!` never merge** — they are clippy/compile errors here by policy.
