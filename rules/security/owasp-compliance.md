---
category: security
scope: [general]
priority: required
applies-to: [all]
---

# OWASP Security Compliance Rules

This document defines mandatory security practices based on the OWASP Top 10 (2021).

---

## A01: Broken Access Control

### CORS Configuration

**NEVER** use wildcard CORS:
```typescript
// WRONG
'Access-Control-Allow-Origin': '*'

// CORRECT
import { getCorsHeaders } from '@app/shared/utils';
const headers = getCorsHeaders(request.headers.get('origin'));
```

**Allowed Origins:**

Configure allowed origins per environment (production, staging, development) and validate incoming origins against a strict allowlist.

---

## A02: Cryptographic Failures

### Random Generation

**NEVER** use `Math.random()` for security purposes:
```typescript
// WRONG - Predictable, only ~5.5 bits entropy per character
password: Math.random().toString(36)

// CORRECT - 122 bits entropy
import { generateSecurePassword } from './utils/securePassword';
password: generateSecurePassword()
```

**Minimum entropy requirements:**
- Temporary passwords: 128 bits
- Session IDs: 128 bits
- CSRF tokens: 128 bits

**Safe alternatives:**
- `crypto.randomUUID()` - 122 bits
- `nanoid(22)` - ~130 bits
- `crypto.getRandomValues()` - Configurable

### Secret Management

- **NEVER** hardcode service role keys in source
- Use environment variables or secret managers
- ANON keys are designed for client-side use but should still come from env vars for rotation
- Run secret scanning tests on every PR

---

## A03: Injection

### SQL Injection Prevention

Use an ORM or query builder for all database queries:
```typescript
// WRONG - Raw SQL with string interpolation
const result = await db.raw(`SELECT * FROM users WHERE id = '${userId}'`);

// CORRECT - ORM with parameterized queries
const result = await db.select().from(users).where(eq(users.id, userId));
```

### XSS Prevention

**NEVER** use `dangerouslySetInnerHTML`:
```typescript
// WRONG
<div dangerouslySetInnerHTML={{ __html: userContent }} />

// CORRECT - Use DOMPurify if HTML is absolutely needed
import DOMPurify from 'dompurify';
<div dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(content) }} />
```

**ESLint Rule:** `no-dangerous-html` enforces this pattern.

---

## A04: Insecure Design

### Error Messages

**NEVER** reveal account existence:
```typescript
// WRONG - Reveals if email exists
if (message.includes("User not found")) {
  return "User not found";  // Attacker knows email doesn't exist
}

// CORRECT - Generic message for all auth failures
return "Invalid credentials";
```

Log detailed errors server-side only.

### Rate Limiting

- Client-side rate limiting is supplementary
- Server-side rate limiting is mandatory
- Authentication endpoints: 5 attempts per 15 minutes
- API endpoints: 100 requests per minute

---

## A05: Security Misconfiguration

### Required Security Headers

All responses MUST include:

| Header | Value | Purpose |
|--------|-------|---------|
| `Strict-Transport-Security` | `max-age=31536000; includeSubDomains; preload` | Force HTTPS |
| `X-Frame-Options` | `DENY` | Clickjacking protection |
| `X-Content-Type-Options` | `nosniff` | MIME sniffing prevention |
| `Referrer-Policy` | `strict-origin-when-cross-origin` | Referrer control |
| `Permissions-Policy` | `camera=(), microphone=(), geolocation=()` | Feature control |
| `Content-Security-Policy` | (See security config) | XSS protection |

---

## A07: Authentication Failures

### JWT Validation

All API endpoints MUST validate JWT:
```typescript
// Edge function example
const authHeader = request.headers.get('Authorization');
if (!authHeader?.startsWith('Bearer ')) {
  return errorResponse(401, 'Missing or invalid Authorization header');
}
const token = authHeader.slice(7);
const { data, error } = await authClient.getUser(token);
if (error || !data.user) {
  return errorResponse(401, 'Invalid token');
}
```

### Session Management

- Use your auth provider's secure defaults for session management
- Logout clears localStorage selectively
- Reserved emails blocked in production

---

## A08: Software and Data Integrity Failures

### Webhook Signature Validation

**ALWAYS** validate signatures with the provider's built-in verification:
```typescript
// CORRECT - Validates signature before parsing
const event = await stripe.webhooks.constructEventAsync(
  payload,
  signature,
  webhookSecret
);

// WRONG - Trusts payload without verification
const event = JSON.parse(payload);
```

**NEVER** parse webhook JSON before signature validation.

### Required Webhook Security Controls

| Control | Purpose | Implementation |
|---------|---------|----------------|
| Signature validation | Verify payload integrity | Provider SDK verification |
| Idempotency check | Prevent duplicate processing | Webhook events table |
| Timestamp validation | Prevent replay attacks | 5-minute window |
| Amount from source | Prevent price manipulation | Use provider session amount |

### Payment Amount Integrity

**NEVER** trust client-submitted payment amounts:
```typescript
// WRONG - Amount from user input
amount: metadata.amount

// CORRECT - Amount from payment provider session
amount: session.amount_total  // Cryptographically signed by provider
```

---

## A09: Logging Failures

### PII Filtering

**NEVER** log sensitive data:
- Passwords
- JWT tokens
- Full credit card numbers
- Social security numbers

```typescript
// WRONG
logger.info('User login', { email, password });

// CORRECT
logger.info('User login', { email, passwordProvided: !!password });
```

### Audit Logging

Log security-relevant events:
- Authentication failures (with IP, user agent)
- Authorization denials
- Configuration changes
- Admin operations

---

## Testing Security

Run security tests before every PR:
```bash
# Run all security tests
bun run test:unit tests/unit/security/
```

---

## References

- [OWASP Top 10 (2021)](https://owasp.org/Top10/)
- [CSP Guide](https://content-security-policy.com/)
