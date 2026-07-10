---
rule: clean-code/general-purpose
title: General-Purpose Over Special-Purpose
category: clean-code
scope: [general]
priority: recommended
applies-to: [typescript, javascript, react]
tags: [architecture, design-patterns, polymorphism, data-driven, extensibility]
---

# General-Purpose Over Special-Purpose

**Rule**: Design mechanisms for broad use cases, not just today's immediate needs.

**Core Idea**: Special cases add complexity. General-purpose solutions are simpler and more maintainable.

---

## The Problem with Special Cases

Every special case adds:
- **Cognitive load**: Developers must remember the special case
- **Code complexity**: More `if` statements, more branches
- **Maintenance burden**: Each special case can break independently
- **Testing overhead**: Must test all combinations

```typescript
// SPECIAL CASES EVERYWHERE
function formatUserName(user: User, context: string): string {
  // Special case 1: Admin users
  if (user.role === 'admin') {
    return `Admin: ${user.name}`;
  }
  
  // Special case 2: Premium users
  if (user.isPremium) {
    return `Premium: ${user.name}`;
  }
  
  // Special case 3: New users (< 7 days)
  if (isNewUser(user)) {
    return `New: ${user.name}`;
  }
  
  // Special case 4: Display context
  if (context === 'header') {
    return user.name.toUpperCase();
  }
  
  // Special case 5: Mobile context
  if (context === 'mobile') {
    return truncate(user.name, 20);
  }
  
  // Default case
  return user.name;
}

// How many combinations? 2^5 = 32 possible states!
// What if admin is also premium and new and on mobile?
```

---

## General-Purpose Solution

Design a mechanism that handles all cases uniformly.

```typescript
// GENERAL-PURPOSE: Configuration-driven
interface UserDisplayConfig {
  prefix?: string;
  transform?: (name: string) => string;
  maxLength?: number;
}

function formatUserName(user: User, config: UserDisplayConfig = {}): string {
  let name = user.name;
  
  // Apply prefix if provided
  if (config.prefix) {
    name = `${config.prefix} ${name}`;
  }
  
  // Apply transformation if provided
  if (config.transform) {
    name = config.transform(name);
  }
  
  // Apply length limit if provided
  if (config.maxLength) {
    name = truncate(name, config.maxLength);
  }
  
  return name;
}

// Usage is explicit and composable
const adminName = formatUserName(user, { prefix: 'Admin' });
const mobileName = formatUserName(user, { maxLength: 20 });
const headerName = formatUserName(user, { transform: (n) => n.toUpperCase() });

// Combinations are explicit, not hidden
const mobileAdmin = formatUserName(user, { 
  prefix: 'Admin', 
  maxLength: 20 
});
```

**Benefits:**
- No hidden special cases
- Explicit configuration
- Easy to add new use cases (just pass different config)
- No code changes needed for new combinations

---

## Eliminate Special Cases

### Strategy 1: Design Normal Case to Handle Edge Cases

```typescript
// SPECIAL CASE: Empty list
function renderUserList(users: User[]): JSX.Element {
  if (users.length === 0) {
    return <EmptyState message="No users found" />;
  }
  
  return (
    <ul>
      {users.map(user => <UserItem key={user.id} user={user} />)}
    </ul>
  );
}

// GENERAL: Normal case handles empty
function UserList({ users }: Props) {
  return (
    <ul>
      {users.map(user => <UserItem key={user.id} user={user} />)}
      {users.length === 0 && <li>No users found</li>}
    </ul>
  );
}
```

### Strategy 2: Use Data to Drive Behavior

```typescript
// SPECIAL CASES: Hardcoded logic for each role
function getUserPermissions(role: string): Permissions {
  if (role === 'admin') {
    return { canEdit: true, canDelete: true, canView: true };
  }
  if (role === 'moderator') {
    return { canEdit: true, canDelete: false, canView: true };
  }
  if (role === 'user') {
    return { canEdit: false, canDelete: false, canView: true };
  }
  return { canEdit: false, canDelete: false, canView: false };
}

// GENERAL: Data-driven
const ROLE_PERMISSIONS: Record<string, Permissions> = {
  admin: { canEdit: true, canDelete: true, canView: true },
  moderator: { canEdit: true, canDelete: false, canView: true },
  user: { canEdit: false, canDelete: false, canView: true },
  guest: { canEdit: false, canDelete: false, canView: false },
};

function getUserPermissions(role: string): Permissions {
  return ROLE_PERMISSIONS[role] ?? ROLE_PERMISSIONS.guest;
}

// Adding new role: just add to data, no code change!
```

### Strategy 3: Polymorphism Over Conditionals

```typescript
// SPECIAL CASES: Type checking everywhere
function processPayment(payment: Payment): Result {
  if (payment.type === 'credit_card') {
    return processCreditCard(payment);
  }
  if (payment.type === 'paypal') {
    return processPayPal(payment);
  }
  if (payment.type === 'bank_transfer') {
    return processBankTransfer(payment);
  }
  throw new Error('Unknown payment type');
}

// GENERAL: Polymorphism
interface PaymentProcessor {
  process(payment: Payment): Promise<Result>;
}

class CreditCardProcessor implements PaymentProcessor {
  async process(payment: Payment): Promise<Result> {
    // Credit card logic
  }
}

class PayPalProcessor implements PaymentProcessor {
  async process(payment: Payment): Promise<Result> {
    // PayPal logic
  }
}

// Registry pattern
const processors = new Map<string, PaymentProcessor>([
  ['credit_card', new CreditCardProcessor()],
  ['paypal', new PayPalProcessor()],
  ['bank_transfer', new BankTransferProcessor()],
]);

function processPayment(payment: Payment): Promise<Result> {
  const processor = processors.get(payment.type);
  if (!processor) {
    throw new Error(`Unknown payment type: ${payment.type}`);
  }
  return processor.process(payment);
}

// Adding new payment type: create new processor, register it
// No changes to processPayment function!
```

---

## Questions to Ask

When designing a feature, ask:

### 1. "What is the simplest interface that covers my current needs?"

Don't add features you don't need yet. But design the interface to be extensible.

```typescript
// TOO SPECIFIC: Only handles current need
function sendWelcomeEmail(userId: string): Promise<void>

// TOO GENERAL: Adds complexity we don't need
function sendEmail(
  userId: string,
  templateId: string,
  variables: Record<string, any>,
  options: EmailOptions
): Promise<void>

// JUST RIGHT: Simple now, extensible later
function sendEmail(
  userId: string,
  template: EmailTemplate,
  options?: EmailOptions
): Promise<void>

// Usage today
await sendEmail(userId, 'welcome');

// Usage tomorrow (when we need variables)
await sendEmail(userId, 'welcome', { userName: 'Alice' });
```

### 2. "What future changes might this need to support?"

Think one step ahead, but don't over-engineer.

### 3. "Can I eliminate this special case with a better general design?"

Often, special cases indicate a design flaw.

```typescript
// SPECIAL CASE: Null handling everywhere
function getUser(id: string): User | null {
  // Returns null if not found
}

// Every caller must handle null
const user = getUser(id);
if (user === null) {
  // Handle not found
}

// GENERAL: Result type handles all cases uniformly
function getUser(id: string): Result<User> {
  // Returns { success: true, data: user } or { success: false, error: '...' }
}

// Uniform handling
const result = getUser(id);
if (!result.success) {
  // Handle error (not found, network error, etc.)
}
```

---

## When Special Cases Are OK

Sometimes special cases are necessary:

### 1. Performance Optimizations

```typescript
// Special case for common path is OK if it's a proven bottleneck
function processData(data: Data[]): Result {
  // Fast path for empty array (common case)
  if (data.length === 0) {
    return { success: true, data: [] };
  }
  
  // General processing
  return processGeneral(data);
}
```

### 2. Backward Compatibility

### 3. Security/Safety

```typescript
// Special case for admin is OK for security
function deleteUser(userId: string, requesterId: string): Result {
  // Special case: Prevent self-deletion
  if (userId === requesterId) {
    return { success: false, error: 'Cannot delete yourself' };
  }
  
  // General deletion logic
  return performDelete(userId);
}
```

---

## Checklist

Before adding a special case:

- [ ] Can the normal case be designed to handle this automatically?
- [ ] Can this be data-driven instead of code-driven?
- [ ] Can polymorphism eliminate the conditional?
- [ ] Is this truly exceptional, or just another use case?
- [ ] Will this special case multiply (admin, then premium, then...)?
- [ ] Is there a general mechanism that handles all cases?

---

## Related Rules

- `obvious-design.md` - Reduce cognitive load
- `deep-modules.md` - Simple interfaces
- `SOLID.md` - Open/Closed Principle

---

**Source**: "A Philosophy of Software Design" by John Ousterhout, Chapter 9
