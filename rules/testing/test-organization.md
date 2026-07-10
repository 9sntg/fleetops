---
category: testing
scope: [general]
applies-to: [typescript, javascript]
---

# Test Organization - Structure, Naming, Files

**Goal**: Consistent, discoverable test organization.

---

## File Structure

```
project-root/
├── src/
│   ├── components/
│   │   ├── Button.tsx
│   │   └── __tests__/
│   │       └── Button.test.tsx          # Co-located component tests
│   ├── services/
│   │   ├── AuthService.ts
│   │   └── __tests__/
│   │       └── AuthService.test.ts      # Co-located unit tests
│   └── utils/
│       ├── pricing.ts
│       └── __tests__/
│           └── pricing.test.ts          # Co-located unit tests
│
├── tests/
│   ├── integration/
│   │   ├── auth/
│   │   │   └── login-flow.test.ts       # Integration tests
│   │   └── checkout/
│   │       └── payment-flow.test.ts
│   │
│   ├── e2e/
│   │   ├── critical-paths.spec.ts       # E2E tests
│   │   └── checkout.spec.ts
│   │
│   └── utils/                            # Shared test utilities
│       ├── factories/
│       │   ├── userFactory.ts
│       │   └── orderFactory.ts
│       ├── fixtures/
│       │   └── mockData.ts
│       └── helpers/
│           ├── testServer.ts
│           └── renderWithProviders.tsx
│
└── package.json
```

---

## File Naming Conventions

| Test Type | Suffix | Location |
|-----------|--------|----------|
| **Unit Tests** | `*.test.ts(x)` | Co-located in `__tests__/` |
| **Integration Tests** | `*.test.ts(x)` | `tests/integration/` |
| **E2E Tests** | `*.spec.ts` | `tests/e2e/` |

### Examples

```
CORRECT:
src/services/auth/__tests__/AuthService.test.ts
tests/integration/auth/login-flow.test.ts
tests/e2e/checkout.spec.ts

INCORRECT:
src/services/auth/AuthService.spec.ts              # Use .test. for unit
tests/integration/auth/login-flow.integration.ts   # Missing .test.
tests/e2e/checkout.test.ts                         # Use .spec. for E2E
```

---

## Test Naming

**Pattern**: `<unit> -- <condition> -> <expected outcome>`

```typescript
// GOOD: Clear and descriptive
describe('calculateDiscount', () => {
  test('valid discount -- applies percentage correctly', () => {
    expect(calculateDiscount(100, 20)).toBe(80);
  });

  test('zero discount -- returns original price', () => {
    expect(calculateDiscount(100, 0)).toBe(100);
  });

  test('invalid discount -- throws error', () => {
    expect(() => calculateDiscount(100, -10)).toThrow();
  });
});

// BAD: Vague or too technical
describe('calculateDiscount', () => {
  test('works', () => { /* ... */ });
  test('test case 1', () => { /* ... */ });
  test('returns 80', () => { /* ... */ }); // Doesn't explain why
});
```

---

## Describe Block Organization

```typescript
describe('Component/Unit Name', () => {
  describe('feature/method name', () => {
    test('specific scenario', () => {
      // ...
    });
  });
});
```

**Example**:

```typescript
describe('UserProfile', () => {
  describe('data loading', () => {
    test('displays user info when loaded', () => { /* ... */ });
    test('shows loading state initially', () => { /* ... */ });
    test('displays error on load failure', () => { /* ... */ });
  });

  describe('editing', () => {
    test('enables save button when data changes', () => { /* ... */ });
    test('submits changes on save click', () => { /* ... */ });
  });
});
```

---

## Co-location Strategy

**Unit tests**: Co-locate with source code

```
src/
├── components/
│   ├── Button.tsx
│   └── __tests__/
│       └── Button.test.tsx    # Same folder as component
```

**Integration/E2E tests**: Centralized in `tests/`

```
tests/
├── integration/               # Multiple units together
│   └── auth/
│       └── login-flow.test.ts
└── e2e/                       # Full app tests
    └── checkout.spec.ts
```

**Why?**
- Co-location: Easy to find related tests, delete together
- Centralized: Integration/E2E tests span multiple units

---

## Shared Utilities

### Factories

```typescript
// tests/utils/factories/userFactory.ts
export function createTestUser(overrides: Partial<User> = {}): User {
  return {
    id: `user-${Date.now()}`,
    name: 'Test User',
    email: `test-${Date.now()}@example.com`,
    role: 'user',
    ...overrides,
  };
}

export function createAdminUser(overrides: Partial<User> = {}): User {
  return createTestUser({ role: 'admin', ...overrides });
}
```

### Fixtures

```typescript
// tests/utils/fixtures/mockData.ts
export const MOCK_USERS: User[] = [
  { id: '1', name: 'User 1', email: 'user1@test.com', role: 'user' },
  { id: '2', name: 'User 2', email: 'user2@test.com', role: 'admin' },
];

export const MOCK_PRODUCTS: Product[] = [
  { id: 'p1', name: 'Product 1', price: 100 },
  { id: 'p2', name: 'Product 2', price: 200 },
];
```

### Helpers

```typescript
// tests/utils/helpers/renderWithProviders.tsx
export function renderWithProviders(
  ui: React.ReactElement,
  options: RenderOptions = {}
) {
  const { initialState = {}, ...renderOptions } = options;

  function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <AuthProvider>
          {children}
        </AuthProvider>
      </QueryClientProvider>
    );
  }

  return render(ui, { wrapper: Wrapper, ...renderOptions });
}
```

---

## Cleanup Pattern

```typescript
// Standard cleanup in test files
beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  vi.resetModules();
  server.resetHandlers();
});

afterAll(() => {
  server.close();
});
```

---

## Checklist

Before merging tests:

- [ ] Test file uses correct suffix (`.test.ts` or `.spec.ts`)?
- [ ] Unit tests co-located in `__tests__/`?
- [ ] Integration tests in `tests/integration/`?
- [ ] E2E tests in `tests/e2e/`?
- [ ] Test names follow pattern: `condition -> outcome`?
- [ ] Describe blocks organized by feature?
- [ ] Using shared factories (not copy-paste)?
- [ ] Proper cleanup in `afterEach`/`afterAll`?
