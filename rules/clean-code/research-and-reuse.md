---
rule: clean-code/research-and-reuse
title: Research & Reuse Before Building
category: clean-code
scope: [general]
priority: recommended
applies-to: [all]
tags: [reuse, research, dependencies, DRY, YAGNI, prior-art]
---

# Research & Reuse Before Building

The macro form of [[DRY]]: before writing any non-trivial new implementation, spend the cheap few minutes to find prior art. Don't reinvent what already exists — in this repo or the wider ecosystem.

## Order of search

1. **In-repo first** — does a utility / module / pattern already do this? (See [[DRY]] — "creating a new file, does it exist?")
2. **GitHub code search** — `gh search repos` / `gh search code` for existing implementations, templates, and patterns before writing anything new.
3. **Package registries** — npm / PyPI / crates.io / etc. Prefer a battle-tested library over hand-rolled utility code.
4. **Primary docs** — confirm API behaviour, package usage, and version-specific details from the vendor's own docs before implementing.
5. **Broader web** — only when the above are insufficient.

## Prefer adoption over net-new

- If an open-source project solves 80%+ of the problem, fork / port / wrap it instead of starting from scratch.
- Port a proven approach rather than writing speculative new code (see [[general-purpose]] / YAGNI).
- Introduce an abstraction only when the repetition is real, not speculative.

## Boundary

When you do reach for a new dependency, honour the host repo's "ask first / new vendor" boundary if it has one — adopting prior art still goes through the project's dependency-approval gate.
