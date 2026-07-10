---
tags:
  - rules
  - agents
  - coding-standards
  - development
created: 2026-04-12
original-source: "F:/s/GlobalRules/file-headers.md"
applies-to: all source files
---

# File Headers

> [!info] Global agent rule
> Every source file must begin with a structured header for instant orientation. Add on creation, update when touching existing files. Bundle header changes with the code change — no separate commits.

Part of [[Global Code Settings]].

---

## When to Add/Update

1. **New files** — add at creation time
2. **Existing files without a header** — add when you first touch the file
3. **Existing files with an outdated header** — update to match current code when modifying

---

## Format

### Python

```python
"""<One-line summary of what this module does.>

Project: <project-name> — <one-line project description>
Module:  <relative path from repo root, e.g. src/pkg/module.py>
Deps:    <key external dependencies this file uses>
Tested:  <path to corresponding test file, or "n/a" if none yet>

Key responsibilities:
- <what this module owns>
- <what it does>

Design constraints:
- <things an agent must not do or must preserve>
"""
```

### TypeScript / JavaScript

```typescript
/**
 * <One-line summary of what this module does.>
 *
 * Project: <project-name> — <one-line project description>
 * Module:  <relative path from repo root>
 * Deps:    <key external dependencies>
 * Tested:  <path to test file, or "n/a">
 *
 * Key responsibilities:
 * - <what this module owns>
 *
 * Design constraints:
 * - <things an agent must not do or must preserve>
 */
```

### Other Languages

Use the language's native block comment syntax with the same fields.

---

## Field Reference

| Field | Purpose |
|---|---|
| **One-line summary** | Instant triage — "is this the file I need?" |
| **Project** | Context if the agent sees the file in isolation |
| **Module** | Canonical path — no guessing relative location |
| **Deps** | Know what's available without scanning all imports |
| **Tested** | Where to find or add tests |
| **Key responsibilities** | Scope boundary — what belongs here vs. elsewhere |
| **Design constraints** | Guard rails the agent must respect |

---

## What to Leave Out

- Author / license / copyright — not useful for agent comprehension
- Version numbers — they drift from the source of truth instantly
- Change history — that's git's job
- Full API documentation — belongs on classes and functions, not the module header
- Boilerplate disclaimers or generated-by notices

---

## Examples

> [!success] Good — concise, useful
> ```python
> """Thin INITECH GraphQL client with OAuth2 password grant.
> 
> Project: initechBridge — INITECH -> Odoo 18 one-way sync
> Module:  src/initech_bridge/clients/initech.py
> Deps:    httpx
> Tested:  tests/test_clients/test_initech.py
> 
> Key responsibilities:
> - OAuth2 token acquisition and refresh
> - GraphQL query execution with error handling
> 
> Design constraints:
> - Read-only: no mutations against INITECH API
> - Query strings live as module constants, not in separate files
> """
> ```

> [!failure] Bad — too generic, no agent value
> ```python
> """This file contains the Initech client."""
> ```

> [!failure] Bad — too verbose, buries the signal
> ```python
> """
> Created by: the maintainer
> Date: 2026-04-12
> Version: 0.1.0
> License: MIT
> ...50 more lines...
> """
> ```
