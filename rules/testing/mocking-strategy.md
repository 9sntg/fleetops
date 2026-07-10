---
category: testing
scope: [general]
applies-to: [typescript, javascript]
---

# Mocking Strategy - Mock Boundaries, Not Your Code

**Golden Rule**: Mock external systems, NEVER your own code.

---

## The Core Philosophy

> **Test what the module DOES, not how it DOES it.**

**Contract** = Public API and expected behavior
**Implementation** = How it works internally

**Test the contract. Mock at boundaries.**

---

## What to Mock (External Boundaries)

### ALWAYS Mock These

**Network calls**:
```typescript
// GOOD: Mock HTTP with MSW
import { setupServer } from 'msw/node';
import { http, HttpResponse } from 'msw';

const server = setupServer(
  http.get('/api/users', () => {
    return HttpResponse.json([
      { id: '1', name: 'User 1' },
      { id: '2', name: 'User 2' },
    ]);
  })
);

beforeAll(() => server.listen());
afterEach(() => server.resetHandlers());
afterAll(() => server.close());
```

**Database**:
```typescript
// GOOD: Mock database client
vi.mock('@app/integrations/database/client', () => ({
  db: {
    from: vi.fn(() => ({
      select: vi.fn().mockResolvedValue({
        data: [{ id: '1', name: 'Test' }],
        error: null,
      }),
    })),
  },
}));
```

**Time/Timers**:
```typescript
// GOOD: Mock time
vi.setSystemTime(new Date('2024-01-01T00:00:00Z'));

// GOOD: Mock timers
vi.useFakeTimers();
setTimeout(() => doSomething(), 1000);
vi.advanceTimersByTime(1000);
vi.useRealTimers();
```

**External libraries**:
```typescript
// GOOD: Mock 3rd party
vi.mock('axios');
vi.mock('ws'); // WebSocket
vi.mock('stripe');
```

---

## What NOT to Mock (Your Code)

### NEVER Mock These

**Your own modules**:
```typescript
// BAD: Mocking your own code
vi.mock('./services/UserService');
vi.mock('./utils/validation');
vi.mock('./helpers/formatters');
```

**Your own functions**:
```typescript
// BAD: Spying on internal functions
import * as helpers from './helpers';
vi.spyOn(helpers, 'internalHelper');
```

**Internal implementation**:
```typescript
// BAD: Testing call order
expect(spy).toHaveBeenCalledBefore(otherSpy);

// BAD: Testing internal calls
expect(internalFunction).toHaveBeenCalled();
```

---

## Why This Matters

### BAD: Brittle, blocks refactoring

```typescript
// WRONG: Testing implementation details
import { findTargetByUrl } from './finder';
import * as url from './utils/url';

test('calls normalizeUrl before matching', () => {
  const spy = vi.spyOn(url, 'normalizeUrl');
  findTargetByUrl(targets, 'localhost:3000');

  expect(spy).toHaveBeenCalled(); // COUPLED TO IMPLEMENTATION!
});

test('tries exact match first', () => {
  const exactMatch = vi.spyOn(finder, 'tryExactMatch');
  findTargetByUrl(targets, 'http://localhost:3000');

  expect(exactMatch).toHaveBeenCalledBefore(containsMatch); // BRITTLE!
});
```

**Problems**:
- Breaks when you rename functions
- Breaks when you change call order
- Doesn't prove behavior works
- Makes refactoring painful

### GOOD: Robust, enables refactoring

```typescript
// RIGHT: Testing behavior/contract
import { findTargetByUrl } from './finder';

describe('Target matching behavior', () => {
  const mockTargets: Target[] = [
    { url: 'http://localhost:3000/', title: 'Home' },
    { url: 'http://localhost:3000/about', title: 'About' },
  ];

  test('finds exact URL match', () => {
    const result = findTargetByUrl(mockTargets, 'http://localhost:3000/');
    expect(result?.url).toBe('http://localhost:3000/');
  });

  test('matches without protocol prefix', () => {
    const result = findTargetByUrl(mockTargets, 'localhost:3000');
    expect(result?.url).toContain('localhost:3000');
  });

  test('returns null when no match found', () => {
    const result = findTargetByUrl(mockTargets, 'nonexistent.com');
    expect(result).toBeNull();
  });
});
```

**Benefits**:
- Tests behavior (input -> output)
- Survives refactoring
- Proves correctness
- No coupling to implementation

---

## Mock at the Boundary

**Visualize your system**:

```
+-------------------------------------+
| Your Application (Don't Mock)       |
|                                     |
|  +----------+    +----------+       |
|  | Service  |--->| Helper   |       |
|  +----------+    +----------+       |
|       |                             |
+-------+-----------------------------+
        |
        v
+-------------------------------------+
| External Boundary (Mock Here!)      |
|                                     |
|  - HTTP APIs                        |
|  - Database                         |
|  - File system                      |
|  - Time/Date                        |
|  - External libraries               |
+-------------------------------------+
```

---

## Integration-Style Tests

**Test the whole flow, not individual steps**:

```typescript
// BAD: Testing each step
test('step 1: normalize URL', () => { /* ... */ });
test('step 2: try exact match', () => { /* ... */ });
test('step 3: try contains match', () => { /* ... */ });

// GOOD: Test the whole flow
test('matches localhost:3000 regardless of format', () => {
  // Don't care HOW, just that it WORKS
  expect(findTargetByUrl(targets, 'localhost:3000')).toBeTruthy();
  expect(findTargetByUrl(targets, 'http://localhost:3000')).toBeTruthy();
  expect(findTargetByUrl(targets, 'https://localhost:3000')).toBeTruthy();
});
```

---

## Use Real Data Structures

```typescript
// BAD: Mocking everything
const mockTarget = {
  url: vi.fn().mockReturnValue('http://localhost:3000'),
  title: vi.fn().mockReturnValue('Test'),
};

// GOOD: Real objects
const mockTarget: Target = {
  url: 'http://localhost:3000',
  title: 'Test',
};
```

**Why**: Real objects behave like production. Mocked objects don't.

---

## The Refactoring Test

Ask yourself:

> **"If I completely rewrote this module but kept the same behavior, would my tests still pass?"**

- If no -> Test is coupled to implementation
- If yes -> Test is coupled to contract

---

## Quick Checklist

Before merging tests:

- [ ] Only external dependencies mocked?
- [ ] No mocking of own code?
- [ ] No spying on internal functions?
- [ ] Tests survive refactoring?
- [ ] Using MSW for HTTP mocks?
- [ ] Using real data structures (not mock objects)?

---

## Real-World Examples

### GOOD: Integration test with MSW

```typescript
// tests/integration/user-profile.test.tsx
import { render, screen, waitFor } from '@testing-library/react';
import { setupServer } from 'msw/node';
import { http, HttpResponse } from 'msw';

const server = setupServer(
  http.get('/api/user/123', () => {
    return HttpResponse.json({
      id: '123',
      name: 'John Doe',
    });
  })
);

beforeAll(() => server.listen());
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

test('loads and displays user', async () => {
  render(<UserProfile userId="123" />);

  await waitFor(() => {
    expect(screen.getByText('John Doe')).toBeInTheDocument();
  });
});
```

**What's mocked**: HTTP API (external boundary)
**What's NOT mocked**: UserProfile component, internal helpers, state management
