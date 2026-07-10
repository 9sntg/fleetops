---
rule: clean-code/deep-modules
title: Deep Modules Principle
category: clean-code
scope: [general]
priority: recommended
applies-to: [typescript, javascript, react]
tags: [architecture, API-design, interfaces, information-hiding, abstraction]
---

# Deep Modules Principle

**Rule**: Modules should have simple interfaces and rich functionality.

**Core Idea**: The best modules hide the most complexity behind the simplest interface.

---

## The Cost-Benefit Equation

Every module has:
- **Cost** = Interface complexity (what callers must understand)
- **Benefit** = Functionality provided

**Goal**: Maximize benefit, minimize cost.

```
Deep Module:     Simple Interface + Rich Implementation = HIGH VALUE
Shallow Module:  Complex Interface + Simple Implementation = LOW VALUE
```

---

## Red Flag: Shallow Module

A module is **shallow** if its interface is complicated relative to its functionality.

### Shallow Module Example

```typescript
// Complex interface for simple functionality
interface DateFormatter {
  setLocale(locale: string): void;
  setTimezone(timezone: string): void;
  setFormat(format: string): void;
  setIncludeTime(include: boolean): void;
  format(date: Date): string;
}

// Usage requires understanding 5 methods for one operation
const formatter = new DateFormatter();
formatter.setLocale('en-US');
formatter.setTimezone('UTC');
formatter.setFormat('YYYY-MM-DD');
formatter.setIncludeTime(false);
const result = formatter.format(new Date());
```

**Problem**: High interface cost (5 methods) for low benefit (format a date).

### Deep Module Example

```typescript
// Simple interface, rich implementation
function formatDate(date: Date, options?: DateFormatOptions): string {
  // Complex logic hidden inside:
  // - Locale detection
  // - Timezone handling
  // - Format parsing
  // - Edge case handling
  // - Caching
  // Total: 200+ lines of implementation
}

// Usage is trivial
const result = formatDate(new Date()); // Uses sensible defaults
const custom = formatDate(new Date(), { locale: 'cs-CZ', includeTime: true });
```

**Benefit**: Low interface cost (1 function, optional config) for high benefit (handles all date formatting complexity).

---

## Information Hiding

The key to deep modules is **hiding complexity**.

### What to Hide

Callers should NOT need to know:
- Implementation details
- Internal data structures
- Execution order of internal steps
- Performance optimizations
- Error recovery mechanisms

### What to Expose

Callers SHOULD know:
- What the module does (high-level behavior)
- What inputs it needs
- What outputs it produces
- What errors it might return

### Example: Supabase Client (Deep Module)

```typescript
// DEEP: Simple interface, hides complexity
const { data, error } = await supabase
  .from('users')
  .select('*')
  .eq('id', userId)
  .single();

// Hidden complexity:
// - HTTP request construction
// - Authentication headers
// - Connection pooling
// - Response parsing
// - Error handling
// - Retry logic
// - Type inference
```

---

## Default to "Doing the Right Thing"

Deep modules should work correctly with minimal configuration.

### Requires Too Much Knowledge

```typescript
// Caller must understand internal implementation
async function saveResponse(
  attemptId: string,
  questionId: string,
  value: number,
  validateInput: boolean,        // Should always be true!
  checkAttemptStatus: boolean,   // Should always be true!
  emitEvent: boolean,            // Should always be true!
  useTransaction: boolean        // Should always be true!
) {
  // ...
}

// Usage is error-prone
await saveResponse(attemptId, questionId, value, true, true, true, true);
```

### Does the Right Thing by Default

```typescript
// Caller only provides essential information
async function saveResponse(
  attemptId: string,
  questionId: string,
  value: number,
  options?: {
    skipValidation?: boolean;    // Rare override
    skipEvents?: boolean;         // For testing only
  }
) {
  // Always validates by default
  // Always checks status by default
  // Always emits events by default
  // Always uses transactions by default
}

// Normal usage is simple
await saveResponse(attemptId, questionId, value);

// Override only when needed
await saveResponse(attemptId, questionId, value, { skipEvents: true });
```

---

## Measuring Depth

### Shallow Module Indicators

- [ ] Many parameters (>5)
- [ ] Many methods in interface (>10)
- [ ] Callers need to read implementation to use it
- [ ] Extensive documentation required
- [ ] Implementation is shorter than documentation
- [ ] Multiple setup steps required before use

### Deep Module Indicators

- [x] Few parameters (1-3, rest optional)
- [x] Small interface (1-5 methods)
- [x] Self-documenting (obvious from signature)
- [x] Works correctly with no configuration
- [x] Implementation much larger than interface
- [x] Single call does meaningful work

---

## Real-World Examples

### Deep: React Query

```typescript
// Simple interface
const { data, isLoading, error } = useQuery({
  queryKey: ['user', userId],
  queryFn: () => fetchUser(userId)
});

// Hides massive complexity:
// - Caching
// - Deduplication
// - Background refetching
// - Stale-while-revalidate
// - Error retry
// - Garbage collection
// - Optimistic updates
```

### Deep: Service Layer

```typescript
// Simple interface
const result = await attemptService.submitAttempt(attemptId);

// Hides complexity:
// - Fetch attempt with responses
// - Validate all responses present
// - Calculate scores across dimensions
// - Update attempt status
// - Emit domain events
// - Handle errors
// - Transaction management
```

### Shallow: Bad API Design

```typescript
// Complex interface for simple task
class EmailSender {
  setSmtpHost(host: string): void;
  setSmtpPort(port: number): void;
  setUsername(user: string): void;
  setPassword(pass: string): void;
  setFromAddress(from: string): void;
  setToAddress(to: string): void;
  setSubject(subject: string): void;
  setBody(body: string): void;
  setHtml(isHtml: boolean): void;
  send(): Promise<void>;
}

// 10 methods to send one email!
```

---

## Refactoring to Deep Modules

### Step 1: Identify Shallow Modules

Look for:
- Functions with many parameters
- Classes with many setters
- APIs requiring multiple calls for one operation
- Code where you need to read the implementation to use it

### Step 2: Combine Related Operations

```typescript
// BEFORE: Shallow (3 separate calls)
await validateAttempt(attemptId);
await calculateScores(attemptId);
await updateAttemptStatus(attemptId, 'completed');

// AFTER: Deep (1 call, hides steps)
await submitAttempt(attemptId);
```

### Step 3: Use Sensible Defaults

```typescript
// BEFORE: Caller must specify everything
function createUser(
  name: string,
  email: string,
  role: string,
  isActive: boolean,
  emailVerified: boolean,
  createdAt: Date
) { }

// AFTER: Defaults handle common case
function createUser(
  name: string,
  email: string,
  options?: {
    role?: string;           // Default: 'user'
    isActive?: boolean;      // Default: true
    emailVerified?: boolean; // Default: false
    createdAt?: Date;        // Default: new Date()
  }
) { }
```

### Step 4: Hide Implementation Details

```typescript
// BEFORE: Exposes internal structure
interface ScoringResult {
  scores: Map<string, number>;           // Internal data structure!
  rawResponses: Response[];              // Internal detail!
  calculationMetadata: CalculationMeta;  // Internal detail!
}

// AFTER: Hides internals
interface ScoringResult {
  getScore(dimension: string): number | undefined;
  getAllScores(): Record<string, number>;
  getDimensionCount(): number;
}
```

---

## Exceptions: When Shallow is OK

Sometimes shallow modules are acceptable:

1. **Adapters/Wrappers**: Thin layer over external API
2. **Type Definitions**: Pure data structures
3. **Constants**: Configuration values
4. **Utilities**: Single-purpose helpers (e.g., `clamp(value, min, max)`)

```typescript
// OK to be shallow: Simple utility
function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}
```

---

## Checklist

Before finalizing a module:

- [ ] Interface is as small as possible?
- [ ] Implementation hides complexity?
- [ ] Works correctly with minimal configuration?
- [ ] Callers don't need to read implementation?
- [ ] Documentation is brief (interface is obvious)?
- [ ] Related operations combined into single calls?
- [ ] Sensible defaults for optional parameters?

---

## Related Rules

- `SOLID.md` - Single Responsibility (one reason to change)
- `DRY.md` - Abstraction quality
- `obvious-design.md` - Reduce cognitive load

---

**Source**: "A Philosophy of Software Design" by John Ousterhout, Chapter 4
