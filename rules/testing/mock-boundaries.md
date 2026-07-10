---
category: testing
scope: [general]
applies-to: [typescript, javascript]
---

# Testing Mock Boundaries

Mock external systems at boundaries, never mock your own code.

---

## Description

The "Don't Mock What You Don't Own" principle states that mocks should only be used for external dependencies (network, filesystem, time) at the system boundary. Mocking internal modules couples tests to implementation details and prevents catching real integration bugs.

---

## Specific Guidelines

### DO:
- Mock HTTP calls with MSW (Mock Service Worker)
- Mock filesystem operations
- Mock timers and Date with vi.useFakeTimers()
- Mock third-party libraries (axios, stripe, etc.)
- Use real implementations of your own code
- Create test doubles for external services at boundary

### DON'T:
- Mock your own modules with vi.mock('./myModule')
- Spy on internal function calls
- Mock React hooks you wrote
- Mock service classes to test controllers
- Mock utility functions
- Use vi.mock() for anything in src/

---

## Implementation Details

### What to Mock (Boundary Diagram):

```
+-------------------------------------------------------------+
|                    YOUR APPLICATION                           |
|                                                              |
|  +----------+    +----------+    +----------+                |
|  |Component |--->| Service  |--->|Repository|                |
|  +----------+    +----------+    +----------+                |
|       |               |               |                      |
|       |               |               |                      |
|       v               v               v                      |
|  +======================================================+   |
|  |           BOUNDARY - MOCK HERE ONLY                   |   |
|  +======================================================+   |
|  |  Network (fetch, axios)                               |   |
|  |  Filesystem (fs)                                      |   |
|  |  Time (Date, setTimeout)                              |   |
|  |  External APIs (Stripe, Supabase, OpenAI)             |   |
|  |  Browser APIs (localStorage, navigator)               |   |
|  +======================================================+   |
+-------------------------------------------------------------+
```

### MSW Setup for API Mocking:

```typescript
// tests/setup/msw-handlers.ts
import { http, HttpResponse } from 'msw';

export const handlers = [
  http.get('/api/users/:id', ({ params }) => {
    return HttpResponse.json({ id: params.id, name: 'Test User' });
  }),
  
  http.post('/api/orders', async ({ request }) => {
    const body = await request.json();
    return HttpResponse.json({ orderId: 'order-123', ...body });
  }),
];
```

---

## Benefits

1. **Catch real bugs**: Integration issues surface in tests
2. **Refactor safely**: Internal changes don't break tests
3. **Realistic testing**: Tests behave like production
4. **Less maintenance**: Fewer mocks to update
5. **Better coverage**: Test actual code paths

---

## Examples

### Correct: MSW for API mocking

```typescript
import { setupServer } from 'msw/node';
import { http, HttpResponse } from 'msw';
import { render, screen, waitFor } from '@testing-library/react';
import { UserProfile } from '@app/components/UserProfile';

const server = setupServer(
  http.get('/api/users/:id', ({ params }) => {
    return HttpResponse.json({
      id: params.id,
      name: 'John Doe',
      email: 'john@example.com',
    });
  })
);

beforeAll(() => server.listen());
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('UserProfile', () => {
  test('displays user data from API', async () => {
    // Uses REAL component, REAL hooks, REAL services
    // Only mocks the network boundary
    render(<UserProfile userId="123" />);

    await waitFor(() => {
      expect(screen.getByText('John Doe')).toBeInTheDocument();
      expect(screen.getByText('john@example.com')).toBeInTheDocument();
    });
  });

  test('handles API errors', async () => {
    server.use(
      http.get('/api/users/:id', () => {
        return HttpResponse.json({ error: 'Not found' }, { status: 404 });
      })
    );

    render(<UserProfile userId="999" />);

    await waitFor(() => {
      expect(screen.getByText(/user not found/i)).toBeInTheDocument();
    });
  });
});
```

### Correct: Mocking timers

```typescript
describe('Session timeout', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  test('expires session after 30 minutes', () => {
    const session = createSession();
    
    expect(session.isValid()).toBe(true);
    
    // Advance time 30 minutes
    vi.advanceTimersByTime(30 * 60 * 1000);
    
    expect(session.isValid()).toBe(false);
  });
});
```

### Correct: Mocking external library

```typescript
import Stripe from 'stripe';

// Mock the external library at boundary
vi.mock('stripe', () => ({
  default: vi.fn().mockImplementation(() => ({
    paymentIntents: {
      create: vi.fn().mockResolvedValue({
        id: 'pi_test_123',
        client_secret: 'secret_123',
        status: 'requires_payment_method',
      }),
    },
  })),
}));

describe('PaymentService', () => {
  test('creates payment intent', async () => {
    // Uses REAL PaymentService, only mocks Stripe
    const service = new PaymentService();
    
    const intent = await service.createPaymentIntent({
      amount: 1000,
      currency: 'usd',
    });
    
    expect(intent.id).toBe('pi_test_123');
  });
});
```

### Correct: Dependency injection for testing

```typescript
// Instead of mocking, inject test doubles
interface IEmailService {
  send(to: string, subject: string, body: string): Promise<void>;
}

class OrderService {
  constructor(private emailService: IEmailService) {}

  async completeOrder(order: Order): Promise<void> {
    await this.saveOrder(order);
    await this.emailService.send(
      order.customerEmail,
      'Order Confirmed',
      `Your order ${order.id} is confirmed`
    );
  }
}

// In tests - inject a fake, don't mock
class FakeEmailService implements IEmailService {
  sentEmails: Array<{ to: string; subject: string; body: string }> = [];

  async send(to: string, subject: string, body: string): Promise<void> {
    this.sentEmails.push({ to, subject, body });
  }
}

test('sends confirmation email', async () => {
  const fakeEmail = new FakeEmailService();
  const orderService = new OrderService(fakeEmail);

  await orderService.completeOrder(testOrder);

  expect(fakeEmail.sentEmails).toHaveLength(1);
  expect(fakeEmail.sentEmails[0].to).toBe(testOrder.customerEmail);
});
```

### Incorrect: Mocking internal modules

```typescript
// BAD: Mocking your own service
vi.mock('@app/services/UserService');

test('controller calls service', async () => {
  const mockUserService = vi.mocked(UserService);
  mockUserService.getUser.mockResolvedValue({ id: '1', name: 'John' });

  const result = await userController.getUser('1');

  // You're testing that controller calls service, not that it works!
  expect(mockUserService.getUser).toHaveBeenCalledWith('1');
});

// GOOD: Test through the real service, mock at network
test('controller returns user data', async () => {
  server.use(
    http.get('/api/users/1', () => HttpResponse.json({ id: '1', name: 'John' }))
  );

  const result = await userController.getUser('1');

  expect(result.name).toBe('John');
});
```

### Incorrect: Mocking utility functions

```typescript
// BAD: Mocking your own utility
vi.mock('@app/utils/formatDate');

test('displays formatted date', () => {
  vi.mocked(formatDate).mockReturnValue('Jan 1, 2024');
  
  render(<DateDisplay date={testDate} />);
  
  // Tests nothing useful - just that mock works
  expect(screen.getByText('Jan 1, 2024')).toBeInTheDocument();
});

// GOOD: Use real utility, test actual output
test('displays formatted date', () => {
  const date = new Date('2024-01-01T00:00:00Z');
  
  render(<DateDisplay date={date} />);
  
  expect(screen.getByText('Jan 1, 2024')).toBeInTheDocument();
});
```

### Incorrect: Spying on internal calls

```typescript
// BAD: Testing implementation details
test('processOrder calls validateOrder', async () => {
  const validateSpy = vi.spyOn(orderService, 'validateOrder');
  
  await orderService.processOrder(testOrder);
  
  expect(validateSpy).toHaveBeenCalled(); // Breaks on refactor!
});

// GOOD: Test the behavior, not the calls
test('processOrder rejects invalid orders', async () => {
  const invalidOrder = { ...testOrder, items: [] };
  
  await expect(orderService.processOrder(invalidOrder)).rejects.toThrow(
    'Order must have at least one item'
  );
});

test('processOrder succeeds with valid order', async () => {
  const result = await orderService.processOrder(validOrder);
  
  expect(result.status).toBe('completed');
});
```

### Incorrect: Mocking React hooks

```typescript
// BAD: Mocking your own hook
vi.mock('@app/hooks/useAuth');

test('shows user name when logged in', () => {
  vi.mocked(useAuth).mockReturnValue({ user: { name: 'John' } });
  
  render(<Header />);
  
  expect(screen.getByText('John')).toBeInTheDocument();
});

// GOOD: Provide auth context, let real hook run
test('shows user name when logged in', () => {
  render(
    <AuthProvider initialUser={{ name: 'John' }}>
      <Header />
    </AuthProvider>
  );
  
  expect(screen.getByText('John')).toBeInTheDocument();
});
```
