---
category: testing
scope: [general]
applies-to: [typescript, javascript]
---

# Test Quality - TEST-SOLID + AAA Pattern

**Goal**: Tests that are readable, reliable, and maintainable.

---

## TEST-SOLID Principles

Custom mnemonic for high-quality tests:

### S -- Simple

**Rule**: Test must be readable in **10 seconds** and understandable.

```typescript
// BAD: Complex, hard to follow
test('complex test', async () => {
  const a = await fetch('/api/x');
  const b = JSON.parse(a);
  if (b.status === 200) {
    const c = b.data.filter(x => x.active);
    const d = c.map(x => x.id);
    expect(d).toContain('123');
  }
});

// GOOD: Clear intent
test('returns active user IDs', async () => {
  const response = await fetchUsers();
  const activeIds = getActiveUserIds(response.data);
  
  expect(activeIds).toContain('123');
});
```

### O -- One Reason to Fail

**Rule**: One test = one main failure reason.

```typescript
// BAD: Multiple unrelated assertions
test('user creation', () => {
  const user = createUser();
  expect(user.id).toBeDefined();
  expect(user.email).toMatch(/@/);
  expect(user.createdAt).toBeInstanceOf(Date);
  expect(database.users).toHaveLength(1); // Different concern!
  expect(audit.logs).toContain('user.created'); // Different concern!
});

// GOOD: Focused tests
test('creates user with valid properties', () => {
  const user = createUser();
  expect(user.id).toBeDefined();
  expect(user.email).toMatch(/@/);
});

test('persists user to database', () => {
  createUser();
  expect(database.users).toHaveLength(1);
});

test('logs user creation event', () => {
  createUser();
  expect(audit.logs).toContain('user.created');
});
```

### L -- Low Duplication

**Rule**: Move repetitive setup to factories and helpers.

```typescript
// BAD: Copy-paste setup
test('test 1', () => {
  const user = { id: '123', name: 'John', email: 'john@test.com', role: 'user' };
  // ...
});

test('test 2', () => {
  const user = { id: '456', name: 'Jane', email: 'jane@test.com', role: 'admin' };
  // ...
});

// GOOD: Use factory
const createUser = (overrides = {}) => ({
  id: `user-${Math.random().toString(36).slice(2)}`,
  name: 'Test User',
  email: 'test@example.com',
  role: 'user',
  ...overrides,
});

test('test 1', () => {
  const user = createUser();
  // ...
});

test('test 2', () => {
  const admin = createUser({ role: 'admin' });
  // ...
});
```

### I -- Isolated from Infrastructure

**Rule**: Unit tests don't communicate with real systems.

```typescript
// BAD: Real database
test('saves user', async () => {
  await realDatabase.save(user); // Slow, flaky, needs cleanup
});

// GOOD: Mocked at boundary
test('saves user', async () => {
  const mockDb = { save: vi.fn() };
  const service = new UserService(mockDb);
  
  await service.createUser(userData);
  
  expect(mockDb.save).toHaveBeenCalledWith(expect.objectContaining(userData));
});
```

### D -- Domain-Oriented

**Rule**: Tests speak the language of your domain.

```typescript
// BAD: Technical jargon
test('inserts row with eq filter', () => {
  db.from('tbl').insert({...}).eq('col', 'val');
});

// GOOD: Domain language
test('creates premium user with discount eligibility', () => {
  const user = createPremiumUser();
  expect(user.canReceiveDiscount()).toBe(true);
});
```

---

## AAA Pattern: Arrange -- Act -- Assert

**Every test should have three clear sections:**

```typescript
test('user can complete checkout', async () => {
  // ===== ARRANGE =====
  const user = createTestUser();
  const product = createTestProduct({ price: 100 });
  await addToCart(user, product);

  // ===== ACT =====
  const order = await checkout(user, {
    cardNumber: '4242424242424242',
    expiry: '12/25',
    cvc: '123',
  });

  // ===== ASSERT =====
  expect(order.status).toBe('completed');
  expect(order.total).toBe(100);
  expect(order.userId).toBe(user.id);
});
```

### Why AAA?

- **Readability**: Clear structure at a glance
- **Maintainability**: Easy to modify one section
- **Debugging**: Quick to find what failed

### Anti-Pattern

```typescript
// BAD: Mixed sections
test('confusing test', async () => {
  const user = createUser();
  const result = await doSomething(user); // Act mixed with arrange
  expect(result).toBeTruthy();
  
  const data = await fetchData(); // More arrange?
  expect(data.length).toBeGreaterThan(0);
  
  await cleanup(); // What is this?
});
```

---

## Test Factories

Avoid manual object creation in every test:

```typescript
// tests/utils/factories/userFactory.ts
export function createTestUser(overrides: Partial<User> = {}): User {
  return {
    id: `user-${Math.random().toString(36).substr(2, 9)}`,
    name: 'Test User',
    email: `test-${Date.now()}@example.com`,
    role: 'user',
    createdAt: new Date().toISOString(),
    ...overrides,
  };
}

// Usage in tests
const user = createTestUser();
const admin = createTestUser({ role: 'admin' });
const specificUser = createTestUser({ id: 'known-id', name: 'John' });
```

---

## Scenario Builders

For complex flows, create scenario helpers:

```typescript
// tests/utils/scenarios/checkoutScenario.ts
export function setupCheckoutScenario() {
  const user = createTestUser();
  const product = createTestProduct({ price: 100 });
  const cart = createTestCart({ userId: user.id, items: [product] });
  
  // Setup mocks
  server.use(
    http.get('/api/cart', () => HttpResponse.json(cart)),
    http.post('/api/checkout', () => HttpResponse.json({ 
      orderId: 'order-123',
      status: 'completed' 
    }))
  );
  
  return { user, product, cart };
}

// In test
test('complete checkout', async () => {
  const { user, cart } = setupCheckoutScenario();
  
  const order = await checkout(user.id, cart.id);
  
  expect(order.status).toBe('completed');
});
```

---

## Checklist

Before merging tests:

- [ ] **S**: Test readable in 10 seconds?
- [ ] **O**: One main failure reason per test?
- [ ] **L**: Using factories (no copy-paste setup)?
- [ ] **I**: Mocked external systems (DB, API)?
- [ ] **D**: Uses domain language?
- [ ] AAA pattern clear (Arrange, Act, Assert)?
- [ ] No mixed arrange/act/assert sections?
- [ ] Scenario builders for complex flows?
