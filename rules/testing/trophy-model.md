---
category: testing
scope: [general]
applies-to: [typescript, javascript]
---

# Testing Trophy Model

Structure tests following Kent C. Dodds' Testing Trophy for maximum confidence with minimal cost.

---

## Description

The Testing Trophy model emphasizes **integration tests** over unit tests, with static analysis as the foundation. This inverts the traditional testing pyramid to focus testing effort where it provides the most confidence: testing real user flows.

---

## Specific Guidelines

### DO:
- Use static analysis (TypeScript, ESLint, Biome) as your first line of defense
- Write integration tests for most features (test real user flows)
- Write unit tests only for complex, non-trivial logic
- Keep E2E tests to critical business paths only
- Test behavior, not implementation details
- Mock at boundaries (network, filesystem), not internal code

### DON'T:
- Write unit tests for trivial code TypeScript already validates
- Create thousands of unit tests that break on every refactor
- Skip integration tests in favor of only unit tests
- Write E2E tests for every edge case (too slow/flaky)
- Mock your own modules or internal functions
- Test implementation details (method calls, internal state)

---

## Implementation Details

### Trophy Layers (Bottom to Top):

```
        / \
       / E2E\        <- 10%: Critical paths only
      /-------\
     /         \
    / INTEGR.   \      <- 50%: THE CUP - Most tests here
   /-------------\
  /   Unit        \   <- 20%: Complex logic only
 /-----------------\
/ Static Analysis   \  <- 40-50%: TypeScript, ESLint, Biome
\-------------------/
```

### Layer Responsibilities:

| Layer | Confidence | Speed | When to Use |
|-------|-----------|-------|-------------|
| Static | 40-50% | Instant | All code |
| Unit | 20% | <1ms | Complex algorithms, edge cases |
| Integration | 50% | 10-100ms | User flows, component trees |
| E2E | 10% | 2-5s+ | Checkout, signup, critical paths |

---

## Benefits

1. **Confidence where it matters**: Integration tests catch real bugs
2. **Refactor-friendly**: Tests survive internal changes
3. **Cost-effective**: Right test type for each scenario
4. **Fast feedback**: Static analysis catches bugs instantly
5. **Less maintenance**: Fewer brittle unit tests

---

## Examples

### Correct: Integration test for user flow

```typescript
// tests/integration/assessment/complete-attempt.test.tsx
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { setupServer } from 'msw/node';
import { http, HttpResponse } from 'msw';

const server = setupServer(
  http.post('/api/assessment/start', () => {
    return HttpResponse.json({ attemptId: 'attempt-123', status: 'started' });
  }),
  http.post('/api/assessment/save', () => {
    return HttpResponse.json({ success: true });
  }),
  http.post('/api/assessment/submit', () => {
    return HttpResponse.json({ status: 'completed', scores: { trust: 85 } });
  })
);

beforeAll(() => server.listen());
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('Assessment Flow Integration', () => {
  test('user completes assessment questionnaire', async () => {
    render(<AssessmentPage />);

    // Start attempt
    await userEvent.click(screen.getByRole('button', { name: /start/i }));
    await waitFor(() => {
      expect(screen.getByText(/question 1/i)).toBeInTheDocument();
    });

    // Answer questions
    await userEvent.click(screen.getByLabelText(/strongly agree/i));
    await userEvent.click(screen.getByRole('button', { name: /next/i }));

    // Submit
    await userEvent.click(screen.getByRole('button', { name: /submit/i }));

    // Verify results
    await waitFor(() => {
      expect(screen.getByText(/your results/i)).toBeInTheDocument();
      expect(screen.getByText(/trust: 85/i)).toBeInTheDocument();
    });
  });

  test('handles API errors gracefully', async () => {
    server.use(
      http.post('/api/assessment/start', () => {
        return HttpResponse.json({ error: 'Server error' }, { status: 500 });
      })
    );

    render(<AssessmentPage />);
    await userEvent.click(screen.getByRole('button', { name: /start/i }));

    await waitFor(() => {
      expect(screen.getByText(/something went wrong/i)).toBeInTheDocument();
    });
  });
});
```

### Correct: Unit test for complex logic only

```typescript
// src/lib/scoring/__tests__/calculateScore.test.ts
import { calculateDimensionScore, normalizeScore } from '../calculateScore';

describe('calculateDimensionScore', () => {
  // Unit test: Complex algorithm with edge cases
  test('calculates weighted average correctly', () => {
    const responses = [
      { questionId: 'q1', value: 5, weight: 2 },
      { questionId: 'q2', value: 3, weight: 1 },
    ];
    
    // (5*2 + 3*1) / (2+1) = 13/3 = 4.33
    expect(calculateDimensionScore(responses)).toBeCloseTo(4.33, 2);
  });

  test('handles empty responses', () => {
    expect(calculateDimensionScore([])).toBe(0);
  });

  test('handles single response', () => {
    const responses = [{ questionId: 'q1', value: 4, weight: 1 }];
    expect(calculateDimensionScore(responses)).toBe(4);
  });
});

describe('normalizeScore', () => {
  // Unit test: Edge cases in normalization
  test('normalizes to 0-100 range', () => {
    expect(normalizeScore(1, 1, 5)).toBe(0);
    expect(normalizeScore(5, 1, 5)).toBe(100);
    expect(normalizeScore(3, 1, 5)).toBe(50);
  });

  test('clamps values outside range', () => {
    expect(normalizeScore(0, 1, 5)).toBe(0);
    expect(normalizeScore(10, 1, 5)).toBe(100);
  });
});
```

### Correct: E2E for critical path only

```typescript
// tests/e2e/checkout.spec.ts
import { test, expect } from '@playwright/test';

test.describe('Checkout Flow', () => {
  // E2E: Only critical business path
  test('complete purchase with Stripe', async ({ page }) => {
    await page.goto('/products/assessment');
    
    await page.click('button:text("Buy Now")');
    await page.waitForURL(/checkout/);
    
    // Fill Stripe elements
    const stripeFrame = page.frameLocator('iframe[name*="stripe"]');
    await stripeFrame.locator('[placeholder="Card number"]').fill('4242424242424242');
    await stripeFrame.locator('[placeholder="MM / YY"]').fill('12/25');
    await stripeFrame.locator('[placeholder="CVC"]').fill('123');
    
    await page.click('button:text("Pay")');
    
    await expect(page.locator('h1')).toHaveText('Thank You!');
    await expect(page.locator('.order-number')).toBeVisible();
  });
});
```

### Incorrect: Over-testing with unit tests

```typescript
// BAD: Unit testing trivial code
describe('User', () => {
  test('has name property', () => {
    const user = { name: 'John' };
    expect(user.name).toBe('John'); // TypeScript already validates this!
  });

  test('name is a string', () => {
    const user: User = { name: 'John' };
    expect(typeof user.name).toBe('string'); // TypeScript handles this!
  });
});

// BAD: Testing implementation details
test('calls fetchUser internally', () => {
  const spy = vi.spyOn(userService, 'fetchUser');
  getUserProfile('123');
  expect(spy).toHaveBeenCalled(); // Breaks on refactor!
});
```

### Incorrect: E2E for edge cases

```typescript
// BAD: Too many E2E tests
test.describe('Form Validation', () => {
  test('shows error for empty email', async ({ page }) => { /* ... */ });
  test('shows error for invalid email', async ({ page }) => { /* ... */ });
  test('shows error for short password', async ({ page }) => { /* ... */ });
  test('shows error for mismatched passwords', async ({ page }) => { /* ... */ });
  // These should be integration tests, not E2E!
});
```

### Incorrect: Mocking internal modules

```typescript
// BAD: Mocking your own code
vi.mock('./utils/formatDate');
vi.mock('./services/UserService');
vi.mock('./hooks/useAuth');

test('formats date correctly', () => {
  // Now you're testing mocks, not real code!
});

// GOOD: Only mock external boundaries
vi.mock('axios'); // External HTTP library
server.use(http.get('/api/users', () => /* ... */)); // MSW for API
```
