---
rule: clean-code/DRY
title: DRY Principles - Don't Repeat Yourself
category: clean-code
scope: [general]
priority: recommended
applies-to: [typescript, javascript, react]
tags: [DRY, abstraction, reuse, constants, refactoring]
---

# DRY Principles - Don't Repeat Yourself

**Golden Rule**: One source of truth for all logic, constants, and configurations.

---

## The DRY Hierarchy (Use This Order)

Before writing ANY new code, follow this hierarchy:

```
SEARCH    -> Does similar code already exist?
              YES -> Use next step
              NO  -> Skip to CREATE

REUSE     -> Can you use existing code as-is?
              YES -> Done! Use it.
              NO  -> Continue

EXTEND    -> Can you modify existing code slightly?
              YES -> Add parameter/variant, done!
              NO  -> Continue

EXTRACT   -> Can you make existing code more reusable?
              YES -> Refactor to shared function/hook
              NO  -> Continue

CREATE    -> Only create new if absolutely necessary
```

---

## Quality Ranking

**Best**: No code (use existing)
**Second best**: Reused code (extend existing)
**Worst**: Duplicate code (recreate logic)

---

## Before Adding ANY Feature

1. **List ALL places** this logic appears
2. **Identify** the most complete implementation
3. **Reuse** that implementation
4. **Extend** if needed, don't recreate

---

## Real-World Examples

### VIOLATES DRY

```typescript
// File 1: DashboardCard.tsx
const SECTION_PADDING = 'py-12 md:py-20';

// File 2: FeatureCard.tsx
const SECTION_PADDING = 'py-12 md:py-20';

// File 3: InfoCard.tsx
const SECTION_PADDING = 'py-12 md:py-20';
```

**Problem**: Config duplicated across 3 files!

### FOLLOWS DRY

```typescript
// constants/responsive-spacing.ts
export const SECTION_PADDING = {
  mobile: 'py-12',
  desktop: 'md:py-20',
} as const;

export function combineSpacing(token: ResponsiveSpacingToken): string {
  return `${token.mobile} ${token.desktop}`;
}

// All components
import { SECTION_PADDING, combineSpacing } from '@app/constants/responsive-spacing';
const padding = combineSpacing(SECTION_PADDING); // "py-12 md:py-20"
```

**Benefit**: Change once, updates everywhere!

---

## DRY Checklist

Before merging code, verify:

- [ ] No duplicated logic introduced?
- [ ] Constants extracted to shared files?
- [ ] Repeated UI patterns componentized?
- [ ] Similar functions consolidated?

---

## Where to Extract

| Duplication Type | Extract To |
|-----------------|------------|
| **Constants** | `/constants/` |
| **Utilities** | `/utils/` or `/features/{feature}/utils/` |
| **Hooks** | `/hooks/` or `/features/{feature}/hooks/` |
| **Components** | `/components/` or `/features/{feature}/components/` |
| **Services** | `/services/` or `/features/{feature}/services/` |

---

## Anti-Patterns to Avoid

```typescript
// BAD: Hardcoded values everywhere
<section className="py-20 mb-20">
<section className="py-16 mb-16">
<section className="py-12 mb-12">

// BAD: Copy-pasted validation
function validateEmail1(email: string) { /* ... */ }
function validateEmail2(email: string) { /* ... */ }

// BAD: Duplicate API calls
async function getUser1() { return fetch('/api/user'); }
async function getUser2() { return fetch('/api/user'); }
```

---

## The Abstraction Quality Test

When extracting shared code, ask these questions to ensure you're creating a **good abstraction**:

### 1. Does this abstraction omit unimportant details?

```typescript
// BAD ABSTRACTION: Exposes internal complexity
function saveUser(
  db: Database,
  tableName: string,
  primaryKey: string,
  userData: Record<string, any>,
  validationRules: ValidationRule[],
  beforeSave?: (data: any) => any,
  afterSave?: (result: any) => void
) {
  // Caller must know too much about internals
}

// GOOD ABSTRACTION: Hides irrelevant details
function saveUser(user: User): Promise<Result<User>> {
  // Internally handles: DB connection, table name, validation, hooks
  // Caller only provides essential information
}
```

### 2. Can someone use it without reading the implementation?

```typescript
// BAD: Must read implementation to use
function processData(items: Item[], mode: number) {
  // What does mode mean? Must read code to find out
  // mode 1 = filter, mode 2 = transform, mode 3 = both?
}

// GOOD: Self-documenting
type ProcessMode = 'filter' | 'transform' | 'both';
function processData(items: Item[], mode: ProcessMode) {
  // Obvious what mode does from the type
}
```

### 3. Is this abstraction simpler than the code it replaces?

```typescript
// BAD: Abstraction is more complex than original
// Before: 3 lines, obvious
const isValid = value > 0 && value < 100;

// After: Must understand abstraction
const isValid = validateRange(value, createRangeValidator(0, 100, { inclusive: false }));

// GOOD: Abstraction simplifies
// Before: 15 lines of complex validation logic repeated everywhere
// After: 1 line, hides complexity
const isValid = isValidResponse(value);
```

### The Three-Question Checklist

Before finalizing an abstraction:

- [ ] **Omits unimportant details?** (Caller doesn't need to know internals)
- [ ] **Usable without reading implementation?** (Self-documenting)
- [ ] **Simpler than the original?** (Reduces complexity, not adds to it)

If you answer "no" to any question, reconsider the abstraction.

### When Abstraction Hurts

Not all code duplication should be eliminated. Sometimes duplication is better than the wrong abstraction.

```typescript
// FORCED ABSTRACTION: These are similar but not the same
function formatUserDisplay(user: User, context: 'header' | 'card' | 'list') {
  // Complex branching logic for different contexts
  // Each context has different requirements
  // Abstraction creates more complexity than it solves
}

// BETTER: Accept some duplication
function formatUserForHeader(user: User): string { }
function formatUserForCard(user: User): JSX.Element { }
function formatUserForList(user: User): string { }
// Each is simple and focused
// Some duplication, but clearer
```

**Principle**: Prefer duplication over the wrong abstraction. You can always extract later when the right abstraction becomes clear.
