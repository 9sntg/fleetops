---
category: testing
scope: [general]
applies-to: [typescript, javascript]
---

# FIRST Principles for Tests

**Acronym**: Fast, Independent, Repeatable, Self-validating, Timely

**Purpose**: Ensure tests are reliable and maintainable.

---

## F -- Fast

**Rule**: Tests must run quickly (seconds, not minutes).

```bash
# GOOD: Unit test suite
bun run test:unit
# Completed in 3.42s

# ACCEPTABLE: Integration tests
bun run test:integration
# Completed in 45s

# SLOW: E2E tests (run separately)
bun run test:e2e
# Completed in 4m 12s
```

**Why**: Fast feedback loop = productive development

**Guidelines**:
- Unit tests: <1ms each
- Integration tests: 10-100ms each
- E2E tests: 2-5s each (run less frequently)

---

## I -- Independent

**Rule**: Tests MUST NOT depend on each other.

### BAD: Tests depend on order

```typescript
let globalUser: User;

test('creates user', () => {
  globalUser = createUser({ name: 'John' });
  expect(globalUser).toBeDefined();
});

test('updates user', () => {
  globalUser.name = 'Jane'; // DEPENDS ON PREVIOUS TEST!
  expect(globalUser.name).toBe('Jane');
});
```

**Problem**: Run tests in isolation -- second test fails!

### GOOD: Independent tests

```typescript
describe('User operations', () => {
  test('creates user', () => {
    const user = createUser({ name: 'John' });
    expect(user).toBeDefined();
  });

  test('updates user', () => {
    const user = createUser({ name: 'John' }); // Own setup!
    user.name = 'Jane';
    expect(user.name).toBe('Jane');
  });
});
```

**Benefit**: Run tests in ANY order, still pass!

---

## R -- Repeatable

**Rule**: Same results every time, everywhere.

### BAD: Non-deterministic tests

```typescript
// Random data
test('generates user ID', () => {
  const id = Math.random().toString(); // DIFFERENT EACH TIME!
  expect(id).toBe('0.12345'); // Fails randomly
});

// Real time
test('checks expiry', () => {
  const now = new Date(); // CHANGES EACH SECOND!
  const expiry = addDays(now, 7);
  expect(expiry.getDate()).toBe(18); // Fails tomorrow
});

// Real network
test('fetches user', async () => {
  const user = await fetch('/api/user/123'); // REAL API!
  expect(user.name).toBe('John'); // Fails if API down
});
```

### GOOD: Deterministic tests

```typescript
// Controlled data
test('generates user ID', () => {
  const id = createTestUserId(); // Factory with seed
  expect(id).toBe('test-user-123');
});

// Mock time
test('checks expiry', () => {
  vi.setSystemTime(new Date('2024-01-01T00:00:00Z')); // FIXED TIME
  const now = new Date();
  const expiry = addDays(now, 7);
  expect(expiry.toISOString()).toBe('2024-01-08T00:00:00.000Z');
});

// Mock network
test('fetches user', async () => {
  server.use(
    http.get('/api/user/123', () => {
      return HttpResponse.json({ id: '123', name: 'John' });
    })
  );

  const user = await fetchUser('123');
  expect(user.name).toBe('John');
});
```

**Benefit**: Same results locally, in CI, today and tomorrow!

---

## S -- Self-Validating

**Rule**: Test either PASSES or FAILS, no manual inspection.

### BAD: Manual verification needed

```typescript
test('logs user data', () => {
  const user = createUser();
  console.log(user); // Must check logs manually
});

test('renders component', () => {
  render(<UserCard user={user} />);
  // No assertions, must visually inspect
});
```

### GOOD: Automated validation

```typescript
test('creates valid user', () => {
  const user = createUser();

  expect(user.id).toBeDefined();
  expect(user.email).toMatch(/^[\w-\.]+@([\w-]+\.)+[\w-]{2,4}$/);
  expect(user.role).toBe('user');
});

test('renders user name', () => {
  render(<UserCard user={user} />);

  expect(screen.getByText('John Doe')).toBeInTheDocument();
  expect(screen.getByText('john@example.com')).toBeInTheDocument();
});
```

**Benefit**: CI knows immediately if something broke!

---

## T -- Timely

**Rule**: Write tests WHEN the problem is fresh in your mind.

### BAD: "I'll test later"

```typescript
// Monday: Write feature
export function calculateDiscount(price: number, percent: number) {
  return price * (1 - percent / 100);
}

// Friday: "Let me write tests now..."
// Problem: Forgot edge cases, requirements, why I wrote it this way
```

### GOOD: Test immediately

```typescript
// Test-Driven Development (TDD)
// 1. Write test first
test('calculates discount', () => {
  expect(calculateDiscount(100, 20)).toBe(80);
});

// 2. Implement to pass test
export function calculateDiscount(price: number, percent: number) {
  return price * (1 - percent / 100);
}

// 3. Add edge cases while fresh
test('handles edge cases', () => {
  expect(calculateDiscount(100, 0)).toBe(100);
  expect(calculateDiscount(100, 100)).toBe(0);
});
```

**Benefit**: Requirements fresh, edge cases obvious, implementation clear!

---

## Cleanup Pattern (Bonus)

**Always cleanup after tests**:

```typescript
// GOOD: Proper cleanup
afterEach(() => {
  vi.clearAllMocks();      // Clear mock calls
  vi.resetModules();       // Reset module state
  server.resetHandlers();  // Reset API mocks
});

afterAll(() => {
  server.close();          // Close mock server
});
```

---

## Quick Checklist

Before merging tests, verify:

- [ ] **F**: Tests run quickly (<1ms unit, <100ms integration)?
- [ ] **I**: Tests run independently (no shared state)?
- [ ] **R**: Tests use deterministic data (no random/time/network)?
- [ ] **S**: Tests have assertions (pass/fail is clear)?
- [ ] **T**: Tests written when feature was fresh?
- [ ] Cleanup in `afterEach`/`afterAll`?

---

## Real-World Violations to Fix

```typescript
// VIOLATES INDEPENDENT
let sharedState = {}; // Tests mutate this!

// VIOLATES REPEATABLE
const userId = Math.random().toString(); // Different each run!

// VIOLATES FAST
await sleep(5000); // Waiting 5 seconds!

// VIOLATES SELF-VALIDATING
console.log('Check if this looks right'); // Manual check needed!
```
