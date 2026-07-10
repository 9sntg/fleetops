---
category: tools
scope: [workflow]
applies-to: [all]
---

# 3-Tier Linting Workflow

**Strategy**: Fast checks during dev, thorough validation before PR.

---

## The Three Tiers

```
Tier 1: Oxlint        -> ~1.2s   (Type-unaware checks)
Tier 2: Biome         -> ~2.9s   (Format + standard lint)
Tier 3: ESLint        -> ~2m 20s (Architectural + type-aware)

Combined fast (1+2): ~4s
Full validation:     ~2m 20s
```

---

## Tier 1: Oxlint (Fast Basic Checks)

**Runtime**: ~1.2 seconds

**What it catches**:
- `no-debugger` - No debugger statements
- `no-eval` - No eval()
- `no-var` - Use let/const
- `prefer-const` - Prefer const for immutable
- `eqeqeq` - Require === and !==
- `no-empty` - No empty blocks
- `no-unsafe-finally` - No flow control in finally
- `no-throw-literal` - Throw Error objects
- `no-alert` - No alert/confirm/prompt
- `typescript/no-unsafe-assignment` - Prevents assigning `any` to typed vars
- `typescript/strict-boolean-expressions` - Restricts types in boolean contexts

**Config**: `.oxlintrc.json`

**Run**:
```bash
bun run lint:ox
```

**When to use**: Included in `bun run lint:fast`

---

## Tier 2: Biome (Format + Standard Lint)

**Runtime**: ~2.9 seconds

**What it catches**:

**Formatting**:
- Inconsistent indentation
- Wrong quote style
- Missing semicolons
- Line width violations

**Linting**:
- `no-console-log` - Console.log warnings
- `no-unused-variables` - Unused variables
- `no-unused-imports` - Unused imports
- `no-implied-eval` - Implied eval
- `no-return-await` - Unnecessary return await
- `noImplicitCoercions` - No shorthand type conversions (`!!value`, `+string`)
- Import organization

**Config**: `biome.json`

**Commands**:
```bash
# Check formatting only
bun run format:check

# Auto-format
bun run format

# Lint only
bun run lint:biome
```

**When to use**: Included in `bun run lint:fast`

---

## Tier 3: ESLint (Architectural Enforcement)

**Runtime**: ~2m 20s (slow - requires full TypeScript compilation)
**Purpose**: Custom rules + deep type checking

**Custom Architectural Plugins** (examples):

### Security Rules
- No `any` on database queries
- Prevent XSS
- Enforce input validation

### Architecture Enforcement
- Enforce repository pattern for data access
- Enforce service layer for business logic
- Enforce auth patterns
- CORS consistency
- Response helpers required

### Configuration Management
- Centralized config
- Secret management

### Safety Rules
- Hook dependency exhaustive checks
- Missing deps detection
- No inline object props causing infinite re-renders
- Error boundaries on pages

**Type-Aware TypeScript Rules**:
- `@typescript-eslint/no-floating-promises` - Unhandled promises
- `@typescript-eslint/await-thenable` - Only await promises
- `@typescript-eslint/no-misused-promises` - Safe promise handling
- `@typescript-eslint/switch-exhaustiveness-check` - Exhaustive switches
- `@typescript-eslint/no-unnecessary-type-assertion` - Redundant assertions
- `@typescript-eslint/strict-boolean-expressions` - Restricts types in boolean contexts
- `@typescript-eslint/no-unsafe-assignment` - Prevents assigning `any` to typed vars

**Config**: `eslint.config.js`

**Commands**:
```bash
# Errors only (faster)
bun run lint:root:errors

# Full lint (warnings + errors)
bun run lint
```

**When to use**: ONLY before creating PR (Phase 2)

---

## Tier Comparison

| Feature | Oxlint | Biome | ESLint |
|---------|--------|-------|--------|
| **Speed** | 1.2s | 2.9s | 2m 20s |
| **Type-aware** | No | No | Yes |
| **Formatting** | No | Yes | No |
| **Custom rules** | No | Limited | Yes |
| **Architecture** | No | No | Yes |
| **Use during dev** | Yes | Yes | No |

---

## Development Workflow

### During Coding (Use Tier 1 + 2)

```bash
# Make changes
vim src/components/UserCard.tsx

# Fast checks (~4s)
bun run lint:fast

# Fix any issues
# Continue coding
```

**What runs**:
1. Oxlint (~1.2s) - Basic checks
2. Biome (~2.9s) - Format + standard lint

**Coverage**: ~80% of issues

---

### Before PR (Add Tier 3)

```bash
# Full validation
bun run lint              # All tiers (~2m 20s)
bun run typecheck         # TypeScript (~30s)
bun run build             # Test build

# Fix all errors
# Create PR
```

**What runs**:
1. Oxlint (~1.2s)
2. Biome (~2.9s)
3. ESLint (~2m 20s) - Architectural + type-aware

**Coverage**: 100% of issues

---

## Why This Approach?

**Problem**: Traditional ESLint-only is too slow (~2m 20s) for rapid iteration.

**Solution**: Multi-tier approach
- Fast feedback during development (4s)
- Thorough validation before PR (2m 20s)
- Best of both worlds!

**Result**:
- Developers stay in flow state
- Architecture rules still enforced
- No compromise on quality
- 97% faster during development

---

## Tool Responsibilities

### Oxlint - The Obvious Stuff
Catches JavaScript/TypeScript basics that are universally wrong.

### Biome - The Style Stuff
Handles formatting and common patterns that should be consistent.

### ESLint - The Architecture Stuff
Enforces project-specific patterns and architectural decisions.

**No overlap** between tiers = maximum efficiency!

---

## TypeScript Compiler Strictness (`tsconfig.json`)

These TypeScript compiler flags are enabled for maximum type safety:

| Flag | Purpose | Alternative |
|------|---------|-------------|
| `verbatimModuleSyntax: true` | Enforces `type` keyword for type-only imports | ESLint: consistent-type-imports |
| `noImplicitReturns: true` | Catches missing return statements | Biome: useGetterReturn (partial) |
| `useUnknownInCatchVariables: true` | Catch variables typed as `unknown` | Biome: noExplicitAny (related) |
| `strict: true` | Enable all strict checks | - |
| `noUncheckedIndexedAccess: true` | Safer array/object access | - |

---

## Quick Reference

| Command | Tiers | Time | Use Case |
|---------|-------|------|----------|
| `bun run lint:ox` | 1 | 1.2s | Manual Tier 1 |
| `bun run lint:biome` | 2 | 2.9s | Manual Tier 2 |
| `bun run lint:fast` | 1+2 | ~4s | **Development** |
| `bun run lint:root:errors` | 3 | 2m 20s | Manual Tier 3 |
| `bun run lint` | 1+2+3 | ~2m 20s | **Before PR** |

---

**Related**:
- `when-to-lint.md` - When to use each tier
