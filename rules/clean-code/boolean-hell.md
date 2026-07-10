---
rule: clean-code/boolean-hell
title: Boolean Hell - Identification & Prevention
category: clean-code
scope: [general]
priority: recommended
applies-to: [typescript, javascript, react]
tags: [state-management, enums, discriminated-unions, boolean, anti-pattern]
---

# Boolean Hell - Identification & Prevention

**Goal**: Replace scattered booleans with clear status enums and derived state.

---

## What is Boolean Hell?

Boolean hell occurs when multiple booleans describe a single process, leading to:
- Impossible/contradictory state combinations
- Complex if/else chains
- Bugs from state drift
- "Add another flag" as the default fix

---

## How to Identify Boolean Hell

### 1. Too Many Booleans for One Process

```typescript
// BAD: Multiple overlapping booleans
const [isTestDone, setIsTestDone] = useState(false);
const [hasSubmitted, setHasSubmitted] = useState(false);
const [isSubmitted, setIsSubmitted] = useState(false);
const [isFinished, setIsFinished] = useState(false);
const [isComplete, setIsComplete] = useState(false);
// Which one is the source of truth?
```

### 2. Impossible/Contradictory Combinations

```typescript
// These combinations shouldn't exist but CAN:
isLoggedIn = false && hasUserId = true
isSubmitting = true && isSubmitted = true
registrationClosed = true && registrationOpen = true
```

### 3. Long, Complex Conditionals

```typescript
// BAD: Nested boolean logic
if (a && b && (c || d) && !e && !f) {
  // "When I add one state, 5 branches break"
}
```

### 4. State in Multiple Places

```typescript
// State scattered across:
// - Component state
// - Redux/Zustand store
// - URL params
// - Backend response
// They drift apart over time
```

### 5. Derived State Stored as State

```typescript
// BAD: Derived state in useState
const [ctaState, setCtaState] = useState('register');

// This is computable from: isLoggedIn, testStatus, flags
// Stored separately - goes stale
```

### 6. No Clear Priority

```typescript
// Two booleans, which wins?
if (showRegistration && showComingSoon) {
  // ??? No clear priority
}
```

### 7. Bug Fixes = "Add Another Boolean"

```typescript
// Instead of fixing the model, just add more flags
const [isSpecialCase, setIsSpecialCase] = useState(false);
const [needsOverride, setNeedsOverride] = useState(false);
// The problem compounds
```

---

## Best Practices

### 1. Distinguish Source of Truth vs Derived State

```typescript
// SOURCE OF TRUTH = facts (from auth, API, database)
const isLoggedIn = !!user;
const testStatus = attempt?.status;
const registrationOpen = featureFlags.registration;

// DERIVED STATE = computed from facts (don't store!)
const ctaState = getCtaState({ isLoggedIn, testStatus, registrationOpen });
```

**Rule**: Derived state should be **computed**, not **stored**.

---

### 2. Replace Booleans with Status Enum

```typescript
// BAD: Multiple booleans
const [isLoading, setIsLoading] = useState(false);
const [hasError, setHasError] = useState(false);
const [isSubmitted, setIsSubmitted] = useState(false);

// GOOD: Single status enum
type LoadingStatus = 'idle' | 'loading' | 'error' | 'success';
const [status, setStatus] = useState<LoadingStatus>('idle');
```

This **eliminates impossible combinations**!

---

### 3. Use Typed Domain States

```typescript
// GOOD: Domain-specific status types
type TestStatus =
  | 'not_started'
  | 'in_progress'
  | 'submitting'
  | 'submitted'
  | 'scored'
  | 'error';

type CtaState =
  | 'register'
  | 'take_test'
  | 'show_result'
  | 'coming_soon'
  | 'closed';

// Map with a pure function
function getCtaState(ctx: Context): CtaState {
  if (!ctx.registrationOpen) return 'closed';
  if (ctx.isComingSoon) return 'coming_soon';
  if (!ctx.isLoggedIn) return 'register';
  if (ctx.testStatus === 'scored') return 'show_result';
  return 'take_test';
}
```

---

### 4. One Place for Decisions + Unit Tests

```typescript
// All logic in ONE function
export function getCtaState(ctx: AppContext): CtaState {
  // Priority order (first match wins)
  if (!ctx.registrationOpen) return 'closed';
  if (ctx.isComingSoon) return 'coming_soon';
  if (!ctx.isLoggedIn) return 'register';
  if (ctx.hasCompletedTest) return 'show_result';
  return 'take_test';
}

// Easy to test with table
describe('getCtaState', () => {
  it.each([
    [{ registrationOpen: false }, 'closed'],
    [{ registrationOpen: true, isComingSoon: true }, 'coming_soon'],
    [{ registrationOpen: true, isLoggedIn: false }, 'register'],
    [{ registrationOpen: true, isLoggedIn: true, hasCompletedTest: true }, 'show_result'],
    [{ registrationOpen: true, isLoggedIn: true, hasCompletedTest: false }, 'take_test'],
  ])('given %o returns %s', (ctx, expected) => {
    expect(getCtaState(ctx)).toBe(expected);
  });
});
```

---

### 5. Clear Priority with Order

```typescript
// GOOD: Priority is the ORDER of checks
function getCtaState(ctx: Context): CtaState {
  // 1. System-level blocks (highest priority)
  if (!ctx.registrationOpen) return 'closed';
  if (ctx.isComingSoon) return 'coming_soon';

  // 2. Auth state
  if (!ctx.isLoggedIn) return 'register';

  // 3. Test completion state
  if (ctx.testStatus === 'scored') return 'show_result';

  // 4. Default
  return 'take_test';
}
```

---

### 6. Standard Async Pattern

```typescript
// Data fetching
type AsyncStatus = 'idle' | 'loading' | 'success' | 'error';

// Form submission
type SubmitStatus = 'editing' | 'submitting' | 'submitted' | 'error';
```

---

### 7. When to Use State Machine (FSM)

Use `useReducer` or XState when:
- Multiple steps and transitions
- Retry/error/loading flows
- Easy to get lost in if-logic

```typescript
// Simple reducer for multi-step flow
type State =
  | { step: 'intro' }
  | { step: 'questions'; index: number }
  | { step: 'submitting' }
  | { step: 'results'; score: number }
  | { step: 'error'; message: string };

type Action =
  | { type: 'START' }
  | { type: 'NEXT_QUESTION' }
  | { type: 'SUBMIT' }
  | { type: 'SUCCESS'; score: number }
  | { type: 'ERROR'; message: string };

function reducer(state: State, action: Action): State {
  switch (state.step) {
    case 'intro':
      if (action.type === 'START') return { step: 'questions', index: 0 };
      break;
    case 'questions':
      if (action.type === 'NEXT_QUESTION') return { step: 'questions', index: state.index + 1 };
      if (action.type === 'SUBMIT') return { step: 'submitting' };
      break;
    // ... etc
  }
  return state;
}
```

---

### 8. Config vs Code

| Put in Config | Put in Code |
|---------------|-------------|
| Labels, icons | Conditions, transitions |
| Links, URLs | Priority rules |
| Tracking events | State logic |
| Copy text | Type definitions |

**Why**: Code gets type-checked and tested. Config is for display.

---

## Quick Audit: "Do I Have Boolean Hell?"

**If 2+ of these apply, refactor to status/FSM:**

- [ ] > 5 booleans for one flow
- [ ] Long ifs with negations: `!a && !b && (c || d)`
- [ ] "This should never happen" states actually happen
- [ ] UI bugs from state combinations
- [ ] Bug fixes = "add another boolean"

---

## Checklist

Before merging state-related code:

- [ ] Using status enum instead of multiple booleans?
- [ ] Derived state computed (not stored)?
- [ ] One function for state decisions?
- [ ] Clear priority order in conditions?
- [ ] Impossible states made impossible by types?
- [ ] Unit tests for state logic?

---

## Examples

**Pattern applied**:
- `CtaState` discriminated union
- `getCtaState(ctx)` pure function
- Table-driven unit tests

---

**Related**:
- `typescript-patterns.md` (discriminated unions)
- `react-patterns.md` (minimal state)
