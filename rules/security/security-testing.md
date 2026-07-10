---
category: security
scope: [general]
priority: required
applies-to: [all]
---

# Security Testing Rules

This document defines how to write security tests following TDD principles.

---

## TDD Security Workflow

Follow **RED -> GREEN -> REFACTOR** for every security fix:

1. **RED**: Write a failing test that exposes the vulnerability
2. **GREEN**: Implement the fix to make the test pass
3. **REFACTOR**: Clean up, update documentation

---

## Required Security Tests

These tests MUST pass on every PR:

| Test File | Purpose | OWASP Category |
|-----------|---------|----------------|
| `SecurePasswordGeneration.test.ts` | No weak RNG | A02 |
| `no-hardcoded-secrets.test.ts` | No service role keys in source | A02 |
| `cors-configuration.test.ts` | No wildcard CORS | A01 |
| `security-guards.unit.test.ts` | Admin role protection | A01 |

### Running Security Tests

```bash
# All security tests
bun run test:unit --grep "security|CORS|auth"

# Individual security tests
bun run test:unit src/core/services/auth/__tests__/SecurePasswordGeneration.test.ts
bun run test:unit src/config/__tests__/no-hardcoded-secrets.test.ts
bun run test:unit tests/unit/security/cors-configuration.test.ts
bun run test:unit src/config/__tests__/security-guards.unit.test.ts
```

---

## Test Patterns

### 1. Test Vulnerability is IMPOSSIBLE

Write tests that verify vulnerabilities cannot exist:

```typescript
// GOOD - Test that weak patterns are impossible
it('does NOT use Math.random pattern', () => {
  const password = generateSecurePassword();
  expect(password).not.toMatch(/^0\.[a-z0-9]+$/);
});

// GOOD - Test that service role keys are never in source
it('config must not contain service_role JWT', async () => {
  const content = await fs.readFile('src/config/sharedEnv.ts', 'utf-8');
  const jwtMatches = content.match(JWT_PATTERN) || [];
  for (const jwt of jwtMatches) {
    const payload = JSON.parse(atob(jwt.split('.')[1]));
    expect(payload.role).not.toBe('service_role');
  }
});
```

### 2. Test All Attack Vectors

Test both positive (allowed) and negative (blocked) cases:

```typescript
// GOOD - Test allowed origins
it.each(ALLOWED_ORIGINS)('allows origin: %s', (origin) => {
  const headers = getCorsHeaders(origin);
  expect(headers['Access-Control-Allow-Origin']).toBe(origin);
});

// GOOD - Test blocked origins
it.each(DISALLOWED_ORIGINS)('rejects origin: %s', (origin) => {
  const headers = getCorsHeaders(origin);
  expect(headers['Access-Control-Allow-Origin']).toBeUndefined();
});
```

### 3. Test Error Message Leakage

Verify error messages don't leak sensitive information:

```typescript
// GOOD - Test generic error messages
it('returns generic message for invalid email', async () => {
  const result = await authService.signIn({
    email: 'nonexistent@example.com',
    password: 'WrongPass123!'
  });
  expect(result.error?.message).toBe('Invalid credentials');
  expect(result.error?.message).not.toContain('not found');
});
```

---

## Mock Boundaries

### What to Mock

Mock external systems:
- Auth provider API
- Payment provider API
- HTTP requests (via MSW)

### What NOT to Mock

Never mock your own code:
- Internal services
- Utility functions
- Security logic

```typescript
// WRONG - Mocking your own security service
vi.mock('./SecurityService');

// CORRECT - Test the real service with mocked external dependencies
const mockAuthClient = createAuthClientMock();
const service = new SecurityService(mockAuthClient);
```

---

## Security Test Categories

### 1. Input Validation Tests

Test that invalid input is rejected:

```typescript
describe('input validation', () => {
  it('rejects XSS payloads', () => {
    const result = validateInput('<script>alert("xss")</script>');
    expect(result.isValid).toBe(false);
  });

  it('rejects SQL injection attempts', () => {
    const result = validateInput("'; DROP TABLE users; --");
    expect(result.isValid).toBe(false);
  });
});
```

### 2. Authentication Tests

Test auth flow security:

```typescript
describe('authentication', () => {
  it('rejects expired tokens', async () => {
    const expiredToken = createExpiredJWT();
    const result = await authService.validateToken(expiredToken);
    expect(result.valid).toBe(false);
  });

  it('rejects tampered tokens', async () => {
    const tamperedToken = createTamperedJWT();
    const result = await authService.validateToken(tamperedToken);
    expect(result.valid).toBe(false);
  });
});
```

### 3. Authorization Tests

Test access control:

```typescript
describe('authorization', () => {
  it('denies admin access to regular users', async () => {
    const user = buildUser({ role: 'user' });
    const result = await authzService.authorizeAdmin({ user });
    expect(result.allowed).toBe(false);
  });

  it('grants access only from app_metadata (not user_metadata)', async () => {
    const maliciousUser = {
      app_metadata: {},
      user_metadata: { role: 'admin' }, // Attacker tried to set own role
    };
    const accessInfo = createUserAccessInfo(maliciousUser);
    expect(accessInfo.isAdmin).toBe(false);
  });
});
```

---

## Regression Tests

Security regression tests prevent reintroduction of vulnerabilities:

```typescript
describe('security regression', () => {
  it('auth service uses secure password generation', async () => {
    const content = await fs.readFile(
      'src/core/services/auth/EmailChangeService.ts',
      'utf-8'
    );
    // Must NOT contain Math.random pattern for password
    const hasWeakPassword = content
      .split('\n')
      .some(line => line.includes('password:') && line.includes('Math.random'));
    expect(hasWeakPassword).toBe(false);
  });
});
```

---

## Continuous Security

### Pre-commit Checks

Add security tests to pre-commit:

```bash
# .husky/pre-commit
bun run test:unit src/config/__tests__/security-guards.unit.test.ts
```

### CI Pipeline

Run all security tests in CI:

```yaml
security-tests:
  script:
    - bun run test:unit --grep "security|CORS|auth"
```

---

## References

- `tests/unit/security/` - Security test directory
- `/rules/testing/` - General testing rules
