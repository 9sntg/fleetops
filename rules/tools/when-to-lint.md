---
category: tools
scope: [workflow]
applies-to: [all]
---

# When to Lint - Fast Dev, Thorough Validation

**CRITICAL**: Do NOT run full `npm run lint` or `bun run typecheck` during active development!

---

## The Two-Phase Workflow

### Phase 1: Active Development (Fast Iteration)

**Use ONLY fast checks** (~4 seconds):

```bash
# After making changes
bun run lint:fast

# This runs:
# - Oxlint (~200ms)  - Fast basic checks + type-aware async safety
# - Biome (~2s)      - Formatting + React hooks + standard lint
```

**What's checked in lint:fast**:
- React hooks rules (`exhaustive-deps`, `rules-of-hooks`) - via Biome
- Async safety (`no-floating-promises`, `await-thenable`, `no-misused-promises`) - via Oxlint type-aware
- Unused imports/variables - via Biome
- Code formatting - via Biome

**DO NOT RUN**:
- `bun run lint` (2m - too slow!)
- `bun run typecheck` (30s - unnecessary during iteration)
- `npm run lint` (even slower!)

**Why**: You need instant feedback to stay in flow state. Full validation interrupts development.

---

### Phase 2: End of Big Refactor (Before PR)

**Run full validation ONLY when done**:

```bash
# 1. Full linting
bun run lint              # ~2m 20s - ESLint with all plugins

# 2. Type checking
bun run typecheck         # ~30s - TypeScript compilation

# 3. Build test
bun run build             # Verify no build errors
```

**When to run Phase 2**:
- Completing a big refactor
- Before creating a pull request
- Before merging to main
- After fixing major architectural issues

**When NOT to run Phase 2**:
- After every small change
- During active coding
- While exploring solutions
- Multiple times per hour

---

## Visual Workflow

```
+----------------------------------------+
| PHASE 1: Active Development            |
| (Repeat 10-50x per session)            |
+----------------------------------------+
|                                        |
|  1. Make code changes                  |
|  2. bun run lint:fast (~4s)            |
|  3. Fix any errors                     |
|  4. Continue coding                    |
|                                        |
|  Repeat until feature/refactor done    |
|                                        |
+----------------------------------------+
                  |
                  v
+----------------------------------------+
| PHASE 2: Pre-PR Validation             |
| (Run 1x when completely done)          |
+----------------------------------------+
|                                        |
|  1. bun run lint (~2m 20s)             |
|  2. bun run typecheck (~30s)           |
|  3. bun run build                      |
|  4. Fix all errors                     |
|  5. Create PR                          |
|                                        |
+----------------------------------------+
```

---

## Testing Workflow

**Same principle as linting: fast during dev, full before PR.**

| Phase | Command | Time | When |
|-------|---------|------|------|
| **During dev** | `bun run test` | ~3 min | After completing a feature/fix |
| **Single file** | `bun run test:full path/to/file` | ~10s | After editing a specific test |
| **Before PR** | `bun run test:full` | ~14 min | Once, when completely done |

**IMPORTANT for AI agents**: Always use `bun run test` (smoke tests) unless the user explicitly asks for full test coverage. Never run `test:full` proactively -- it's too slow for iterative work.

---

## What Each Tool Catches

### Fast Checks (Phase 1)

**Oxlint** (~1.2s):
- Obvious JS/TS mistakes
- `no-var`, `no-debugger`, `no-eval`
- Simple syntax issues

**Biome** (~2.9s):
- Code formatting
- Unused variables (warnings)
- Unused imports (warnings)
- Basic style issues

**Coverage**: ~80% of common issues

---

### Full Validation (Phase 2)

**ESLint** (~2m 20s):
- Custom architectural rules
- Database access patterns
- Repository pattern enforcement
- Security checks
- Type-aware TypeScript rules

**TypeScript** (~30s):
- Full type compilation
- Type errors across codebase
- Strict mode checks

**Coverage**: 100% of all issues

---

## Red Flags

**Signs you're doing it WRONG**:

```bash
# Running full lint after every change
$ vim UserCard.tsx
$ bun run lint        # 2m 20s - WRONG!
$ vim UserCard.tsx
$ bun run lint        # 2m 20s - WRONG!
$ vim UserCard.tsx
$ bun run lint        # 2m 20s - WRONG!
```

**This destroys productivity!**

**Signs you're doing it RIGHT**:

```bash
# Fast iteration
$ vim UserCard.tsx
$ bun run lint:fast   # 4s - GOOD!
$ vim UserCard.tsx
$ bun run lint:fast   # 4s - GOOD!
$ vim UserCard.tsx
$ bun run lint:fast   # 4s - GOOD!

# ... 30 iterations later ...

# Final validation (once!)
$ bun run lint        # 2m 20s - GOOD!
$ bun run typecheck   # 30s - GOOD!
$ bun run build       # Test build
$ git add . && git commit
```

---

## IDE Integration (Optional)

**For instant feedback during coding**:

VS Code settings:
```json
{
  "editor.formatOnSave": true,
  "editor.codeActionsOnSave": {
    "source.fixAll.biome": true
  },
  "[typescript]": {
    "editor.defaultFormatter": "biomejs.biome"
  },
  "[typescriptreact]": {
    "editor.defaultFormatter": "biomejs.biome"
  }
}
```

**Benefit**: Auto-fix on save, no need to run commands!

---

## CI/CD (Automated)

**GitHub Actions runs full validation automatically**:

```yaml
# Fast checks first (parallel)
- Oxlint
- Biome
- Format check

# Slow checks after (parallel)
- ESLint root
- ESLint admin
- TypeScript root
- TypeScript admin
```

**You don't need to run these locally** - CI catches them!

---

## Exception: Big Refactor Checkpoints

**During multi-hour refactors**, consider checkpoint validation every ~2 hours:

```bash
# After 2 hours of refactoring
$ bun run typecheck     # Quick sanity check

# Continue refactoring...

# After another 2 hours
$ bun run typecheck     # Another checkpoint

# When completely done
$ bun run lint          # Full validation
```

**Why**: Catch type errors before they compound. But still don't run full lint!

---

## Quick Reference

| Scenario | Command | Time |
|----------|---------|------|
| **After small change** | `bun run lint:fast` | ~4s |
| **After medium change** | `bun run lint:fast` | ~4s |
| **After big change** | `bun run lint:fast` | ~4s |
| **Checkpoint (2hr refactor)** | `bun run typecheck` | ~30s |
| **Before PR** | `bun run lint` | ~2m 20s |
| **Before PR** | `bun run typecheck` | ~30s |
| **Before merge** | `bun run build` | varies |

---

## Golden Rules

1. **Fast checks during development** - Always
2. **Full validation before PR** - Once
3. **Never interrupt flow with slow checks** - Never
4. **Trust CI to catch issues** - Yes

---

**Related**:
- `linting-workflow.md` - Detailed tool breakdown
