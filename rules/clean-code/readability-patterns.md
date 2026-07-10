---
rule: clean-code/readability-patterns
title: Readability Patterns - Clear, Scannable Code
category: clean-code
scope: [general]
priority: recommended
applies-to: [typescript, javascript, react]
tags: [readability, guard-clauses, conditionals, naming, early-return]
---

# Readability Patterns - Clear, Scannable Code

**Goal**: Code that reads like prose, with minimal cognitive load.

---

## Early Returns (Guard Clauses)

**What**: Exit early when preconditions fail.
**Why**: Less nesting, clearer happy path.

```typescript
// BAD: Nested conditions
function processItem(item?: Item) {
  if (item) {
    if (item.active) {
      if (item.valid) {
        // main logic buried deep
      }
    }
  }
}

// GOOD: Guard clauses
function processItem(item?: Item) {
  if (!item) return;           // guard
  if (!item.active) return;    // guard
  if (!item.valid) return;     // guard

  // main logic at top level
}
```

---

## Extract Helper Functions

**What**: Move complex checks into descriptively named helpers.
**Why**: Caller reads like prose; helpers are testable.

```typescript
// BAD: Inline complex logic
if (user.age > 18 && user.membership === 'premium' && !user.banned) {
  applyDiscount(user);
}

// GOOD: Extracted helper
function isEligibleForDiscount(u: User): boolean {
  return u.age > 18 && u.membership === 'premium' && !u.banned;
}

if (isEligibleForDiscount(user)) {
  applyDiscount(user);
}
```

**Naming Rule**: Name after **intent**, not mechanics.
- `isEligibleForDiscount()` - describes WHY
- `checkUserProperties()` - describes HOW

---

## Ternaries for Simple Conditions

**What**: Concise expression-level branching.
**Why**: Keeps trivial cases inline; avoid nesting.

```typescript
// GOOD: Simple, single-line
const statusLabel = isActive ? 'Active' : 'Inactive';
const displayName = user.nickname ?? user.name;

// BAD: Nested ternaries = hard to read
const result = a ? (b ? 'x' : 'y') : (c ? 'z' : 'w');

// BAD: Complex logic in ternary
const price = user.isPremium && order.total > 100 
  ? calculatePremiumDiscount(order) 
  : calculateStandardPrice(order); // Too complex!
```

**Rule**: If ternary needs parentheses or spans multiple lines, use `if/else`.

---

## Simplify Complex Conditions

**What**: Break boolean logic into named parts.
**Why**: Names communicate intent.

```typescript
// BAD: Long boolean chain
if (age >= 18 && user.role === 'admin' && !user.suspended && user.verified) {
  grantAccess();
}

// GOOD: Named conditions
const isAdult = age >= 18;
const isAdmin = user.role === 'admin';
const isActiveAccount = !user.suspended && user.verified;

if (isAdult && isAdmin && isActiveAccount) {
  grantAccess();
}
```

---

## Use `switch` for Single Discriminator

**What**: `switch` instead of `if/else` chains when branching on one variable.
**Why**: Clearer structure, easy to extend (Open/Closed).

```typescript
// BAD: if/else chain
function getLabel(status: Status): string {
  if (status === 'active') return 'Active';
  if (status === 'inactive') return 'Inactive';
  if (status === 'pending') return 'Pending';
  return 'Unknown';
}

// GOOD: switch with exhaustive check
function getLabel(status: Status): string {
  switch (status) {
    case 'active':   return 'Active';
    case 'inactive': return 'Inactive';
    case 'pending':  return 'Pending';
    default: {
      const _exhaustive: never = status;
      throw new Error(`Unhandled: ${_exhaustive}`);
    }
  }
}
```

---

## Refactor Nested Conditions

**What**: Flatten using guards or combined predicates.
**Why**: Reduces cognitive load.

```typescript
// BAD: Deeply nested
if (user) {
  if (user.isActive) {
    if (user.hasPermission) {
      doThing();
    }
  }
}

// GOOD: Optional chaining
if (user?.isActive && user?.hasPermission) {
  doThing();
}

// GOOD: Early returns
if (!user) return;
if (!user.isActive) return;
if (!user.hasPermission) return;

doThing();
```

---

## Logical AND for Conditional Rendering

**What**: Use `&&` for simple conditional rendering in JSX.
**Why**: Concise, readable for simple cases.

```typescript
// GOOD: Simple conditional
{isLoggedIn && <Dashboard />}
{hasError && <ErrorMessage error={error} />}

// BAD: Ternary when one branch is null
{isLoggedIn ? <Dashboard /> : null}

// CAUTION: Watch for falsy values
{count && <Badge count={count} />} // Renders "0" if count=0!

// FIX: Explicit boolean
{count > 0 && <Badge count={count} />}
```

---

## Quick Reference

| Pattern | Use When |
|---------|----------|
| **Early returns** | Preconditions, validation, null checks |
| **Helper functions** | Complex boolean logic, reusable checks |
| **Ternaries** | Simple, one-line expressions |
| **Named conditions** | Multiple boolean parts to combine |
| **switch** | Branching on single variable |
| **Optional chaining** | Nested object access |
| **Logical AND** | Simple conditional rendering |

---

## Checklist

Before merging code:

- [ ] No deeply nested conditions (max 2 levels)?
- [ ] Complex logic extracted to named helpers?
- [ ] Guards/early returns for preconditions?
- [ ] Ternaries only for simple cases?
- [ ] Switch with exhaustive check for discriminators?
