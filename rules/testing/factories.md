---
category: testing
scope: [general]
applies-to: [typescript, javascript]
---

# Testing Factories and Builders

Use factories for test data and scenario builders for complex test setups.

---

## Description

Test factories create consistent, valid test objects with sensible defaults. Scenario builders compose multiple factories and mocks to set up complex test scenarios. Both reduce duplication and make tests more readable and maintainable.

---

## Specific Guidelines

### DO:
- Create factories for each domain entity
- Provide sensible defaults in factories
- Allow overriding specific properties
- Use scenario builders for multi-step setups
- Co-locate factories in `tests/utils/factories/`
- Type factories properly for IDE support

### DON'T:
- Inline object creation in every test
- Copy-paste test data between tests
- Create incomplete/invalid test objects
- Use random data without control
- Put factory logic inside test files
- Create overly complex factory hierarchies

---

## Implementation Details

### Factory Pattern:

```typescript
// tests/utils/factories/userFactory.ts
export function createUser(overrides: Partial<User> = {}): User {
  return {
    id: `user-${crypto.randomUUID().slice(0, 8)}`,
    email: `user-${Date.now()}@example.com`,
    name: 'Test User',
    role: 'user',
    createdAt: new Date(),
    ...overrides,
  };
}
```

### Scenario Builder Pattern:

```typescript
// tests/utils/scenarios/checkoutScenario.ts
export function setupCheckoutScenario(options: CheckoutOptions = {}) {
  const user = createUser(options.user);
  const products = options.products ?? [createProduct(), createProduct()];
  const cart = createCart({ userId: user.id, items: products });
  
  // Setup API mocks
  server.use(
    http.get('/api/cart', () => HttpResponse.json(cart)),
    http.post('/api/checkout', () => HttpResponse.json({ orderId: 'order-123' }))
  );
  
  return { user, products, cart };
}
```

---

## Benefits

1. **DRY**: No duplicated test data
2. **Maintainable**: Change defaults in one place
3. **Readable**: Tests focus on what matters
4. **Valid**: Factories ensure complete objects
5. **Flexible**: Override only what's relevant

---

## Examples

### Correct: Entity factories

```typescript
// tests/utils/factories/assessmentFactory.ts
import { AssessmentAttempt, AssessmentResponse, AssessmentScore } from '@app/types/assessment';

export function createAssessmentAttempt(
  overrides: Partial<AssessmentAttempt> = {}
): AssessmentAttempt {
  const id = overrides.id ?? `attempt-${Date.now()}`;
  
  return {
    id,
    userId: `user-${Date.now()}`,
    pairId: null,
    status: 'started',
    role: 'self',
    responses: [],
    startedAt: new Date(),
    completedAt: null,
    score: null,
    ...overrides,
  };
}

export function createAssessmentResponse(
  overrides: Partial<AssessmentResponse> = {}
): AssessmentResponse {
  return {
    id: `response-${Date.now()}`,
    attemptId: `attempt-${Date.now()}`,
    questionId: 'q1',
    dimension: 'trust',
    value: 4,
    answeredAt: new Date(),
    ...overrides,
  };
}

export function createAssessmentScore(
  overrides: Partial<AssessmentScore> = {}
): AssessmentScore {
  return {
    attemptId: `attempt-${Date.now()}`,
    dimension: 'trust',
    score: 75,
    percentile: 80,
    ...overrides,
  };
}

// Composite factory for complete attempt with responses
export function createCompletedAttempt(
  overrides: Partial<AssessmentAttempt> = {}
): AssessmentAttempt {
  const attemptId = overrides.id ?? `attempt-${Date.now()}`;
  
  return createAssessmentAttempt({
    id: attemptId,
    status: 'completed',
    completedAt: new Date(),
    responses: [
      createAssessmentResponse({ attemptId, dimension: 'trust', value: 4 }),
      createAssessmentResponse({ attemptId, dimension: 'communication', value: 5 }),
      createAssessmentResponse({ attemptId, dimension: 'intimacy', value: 3 }),
    ],
    score: 78,
    ...overrides,
  });
}
```

### Correct: Using factories in tests

```typescript
describe('AssessmentService', () => {
  test('calculates average score from responses', () => {
    // Only specify what matters for this test
    const attempt = createAssessmentAttempt({
      responses: [
        createAssessmentResponse({ value: 4 }),
        createAssessmentResponse({ value: 5 }),
        createAssessmentResponse({ value: 3 }),
      ],
    });

    const avgScore = calculateAverageScore(attempt);

    expect(avgScore).toBe(4);
  });

  test('marks attempt as completed', () => {
    const attempt = createAssessmentAttempt({ status: 'started' });

    const completed = markAsCompleted(attempt);

    expect(completed.status).toBe('completed');
    expect(completed.completedAt).toBeInstanceOf(Date);
  });

  test('rejects already completed attempts', () => {
    // Use composite factory for completed state
    const attempt = createCompletedAttempt();

    expect(() => submitResponses(attempt, [])).toThrow('Already completed');
  });
});
```

### Correct: Scenario builder

```typescript
// tests/utils/scenarios/pairAssessmentScenario.ts
import { setupServer } from 'msw/node';
import { http, HttpResponse } from 'msw';

interface PairAssessmentScenarioOptions {
  selfCompleted?: boolean;
  partnerCompleted?: boolean;
  pairScoreCalculated?: boolean;
}

export function setupPairAssessmentScenario(
  options: PairAssessmentScenarioOptions = {}
) {
  const {
    selfCompleted = false,
    partnerCompleted = false,
    pairScoreCalculated = false,
  } = options;

  // Create entities
  const user = createUser();
  const partner = createUser({ name: 'Partner' });
  const pair = createPair({ user1Id: user.id, user2Id: partner.id });

  const selfAttempt = selfCompleted
    ? createCompletedAttempt({ userId: user.id, pairId: pair.id, role: 'self' })
    : createAssessmentAttempt({ userId: user.id, pairId: pair.id, role: 'self' });

  const partnerAttempt = partnerCompleted
    ? createCompletedAttempt({ userId: partner.id, pairId: pair.id, role: 'partner' })
    : createAssessmentAttempt({ userId: partner.id, pairId: pair.id, role: 'partner' });

  const pairScore = pairScoreCalculated
    ? createPairScore({ pairId: pair.id })
    : null;

  // Setup API mocks
  const handlers = [
    http.get('/api/pairs/:id', () => HttpResponse.json(pair)),
    http.get('/api/assessment/attempts', () =>
      HttpResponse.json([selfAttempt, partnerAttempt])
    ),
  ];

  if (pairScore) {
    handlers.push(
      http.get('/api/assessment/pair-score/:pairId', () =>
        HttpResponse.json(pairScore)
      )
    );
  }

  return {
    user,
    partner,
    pair,
    selfAttempt,
    partnerAttempt,
    pairScore,
    handlers,
  };
}

// Usage in tests
describe('Pair Assessment Results', () => {
  test('shows waiting state when partner not completed', () => {
    const scenario = setupPairAssessmentScenario({
      selfCompleted: true,
      partnerCompleted: false,
    });
    server.use(...scenario.handlers);

    render(<PairResults pairId={scenario.pair.id} />);

    expect(screen.getByText(/waiting for partner/i)).toBeInTheDocument();
  });

  test('shows pair score when both completed', () => {
    const scenario = setupPairAssessmentScenario({
      selfCompleted: true,
      partnerCompleted: true,
      pairScoreCalculated: true,
    });
    server.use(...scenario.handlers);

    render(<PairResults pairId={scenario.pair.id} />);

    expect(screen.getByText(/your pair score/i)).toBeInTheDocument();
  });
});
```

### Correct: Factory with relationships

```typescript
// tests/utils/factories/orderFactory.ts
export function createOrder(overrides: Partial<Order> = {}): Order {
  const id = overrides.id ?? `order-${Date.now()}`;
  const userId = overrides.userId ?? `user-${Date.now()}`;
  
  return {
    id,
    userId,
    status: 'pending',
    items: overrides.items ?? [createOrderItem({ orderId: id })],
    total: 0, // Will be calculated
    createdAt: new Date(),
    ...overrides,
  };
}

export function createOrderItem(overrides: Partial<OrderItem> = {}): OrderItem {
  return {
    id: `item-${Date.now()}`,
    orderId: `order-${Date.now()}`,
    productId: `product-${Date.now()}`,
    quantity: 1,
    price: 100,
    ...overrides,
  };
}

// Factory that calculates derived values
export function createOrderWithTotal(overrides: Partial<Order> = {}): Order {
  const order = createOrder(overrides);
  order.total = order.items.reduce((sum, item) => sum + item.price * item.quantity, 0);
  return order;
}
```

### Incorrect: Inline object creation

```typescript
// BAD: Duplicated, verbose, easy to miss fields
describe('OrderService', () => {
  test('calculates total', () => {
    const order = {
      id: 'order-1',
      userId: 'user-1',
      status: 'pending',
      items: [
        { id: 'item-1', productId: 'prod-1', quantity: 2, price: 50 },
        { id: 'item-2', productId: 'prod-2', quantity: 1, price: 100 },
      ],
      createdAt: new Date(),
    };

    expect(calculateTotal(order)).toBe(200);
  });

  test('validates order', () => {
    const order = {
      id: 'order-2',
      userId: 'user-2',
      status: 'pending',
      items: [], // Only difference!
      createdAt: new Date(),
    };

    expect(() => validateOrder(order)).toThrow();
  });
});

// GOOD: Use factories
describe('OrderService', () => {
  test('calculates total', () => {
    const order = createOrder({
      items: [
        createOrderItem({ quantity: 2, price: 50 }),
        createOrderItem({ quantity: 1, price: 100 }),
      ],
    });

    expect(calculateTotal(order)).toBe(200);
  });

  test('validates order', () => {
    const order = createOrder({ items: [] });

    expect(() => validateOrder(order)).toThrow();
  });
});
```

### Incorrect: Incomplete test objects

```typescript
// BAD: Missing required fields, type errors suppressed
test('processes user', () => {
  const user = { name: 'John' } as User; // Missing id, email, etc!
  
  processUser(user); // May crash on missing fields
});

// GOOD: Factory ensures complete, valid object
test('processes user', () => {
  const user = createUser({ name: 'John' });
  
  processUser(user); // All required fields present
});
```
