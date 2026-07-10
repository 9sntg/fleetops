---
rule: clean-code/obvious-design
title: Obvious Design Principle
category: clean-code
scope: [general]
priority: recommended
applies-to: [typescript, javascript, react]
tags: [readability, cognitive-load, naming, dependencies, complexity]
---

# Obvious Design Principle

**Rule**: Code should be obvious. Readers should quickly understand what it does and how to change it.

**Core Idea**: If a developer needs to guess, dig, or experiment to understand code, it's not obvious enough.

---

## The Three Symptoms of Complexity

### 1. Change Amplification
A seemingly simple change requires modifications in many places.

```typescript
// NOT OBVIOUS: Changing timeout requires 5 file edits
// File 1: api/users.ts
const TIMEOUT = 5000;

// File 2: api/posts.ts
const TIMEOUT = 5000;

// File 3: api/comments.ts
const TIMEOUT = 5000;
```

```typescript
// OBVIOUS: Single source of truth
// config/api.ts
export const API_TIMEOUT = 5000;

// All files import from one place
import { API_TIMEOUT } from '@app/config/api';
```

### 2. Cognitive Load
How much a developer needs to know to complete a task.

```typescript
// HIGH COGNITIVE LOAD: Must remember order, side effects, global state
let currentUser: User | null = null;
let isAuthenticated = false;
let sessionToken = '';

function login(email: string, password: string) {
  // Must call in this exact order!
  validateCredentials(email, password);
  currentUser = fetchUser(email);
  isAuthenticated = true;
  sessionToken = generateToken();
  initializeSession();
  // If you forget any step, bugs appear
}
```

```typescript
// LOW COGNITIVE LOAD: Encapsulated, obvious
class AuthSession {
  private user: User | null = null;
  private token: string = '';

  async login(email: string, password: string): Promise<Result<User>> {
    // All steps handled internally
    // Caller only needs to know: login(email, password)
  }

  isAuthenticated(): boolean {
    return this.user !== null;
  }
}
```

### 3. Unknown Unknowns (THE WORST)
Things you need to know but have no way to discover.

```typescript
// UNKNOWN UNKNOWN: Hidden dependency
export function saveResponse(attemptId: string, value: number) {
  // BUG: This function assumes initialize() was called first!
  // Nothing in the signature tells you this.
  // You'll only discover this when it crashes in production.
  const config = getGlobalConfig(); // Throws if not initialized!
  // ...
}
```

```typescript
// OBVIOUS: Dependency is explicit
export function saveResponse(
  attemptId: string,
  value: number,
  config: AppConfig  // Explicit dependency!
) {
  // Now it's obvious what's needed
}
```

---

## Making Code Obvious

### 1. Consistent Naming

Use the same name for the same concept everywhere.

```typescript
// INCONSISTENT: Same concept, different names
function getUserData(id: string) { }
function fetchUserInfo(id: string) { }
function retrieveUserDetails(id: string) { }
function loadUserProfile(id: string) { }
// Which one should I use? What's the difference?
```

```typescript
// CONSISTENT: Same pattern everywhere
function getUser(id: string) { }
function getPost(id: string) { }
function getComment(id: string) { }
// Pattern is obvious: get{Entity}(id)
```

### 2. Explicit Dependencies

Make all dependencies visible in the function signature.

```typescript
// HIDDEN DEPENDENCIES
import { supabase } from '@app/lib/supabase'; // Global!

export class UserService {
  async getUser(id: string) {
    // Hidden dependency on global supabase
    return supabase.from('users').select().eq('id', id);
  }
}
```

```typescript
// EXPLICIT DEPENDENCIES
export class UserService {
  constructor(private db: SupabaseClient) {} // Explicit!

  async getUser(id: string) {
    return this.db.from('users').select().eq('id', id);
  }
}

// Usage makes dependencies obvious
const userService = new UserService(supabase);
```

### 3. Colocation of Related Code

Put related code together so readers see the full picture.

```typescript
// SCATTERED: Related logic in different files
// File: constants/app.ts
export const MIN_RESPONSES = 10;

// File: validators/app.ts
export function validateResponses(count: number) {
  return count >= 10; // Magic number! Where did 10 come from?
}

// File: services/app.ts
export function canSubmit(responses: Response[]) {
  return responses.length >= 10; // Another magic 10!
}
```

```typescript
// COLOCATED: Related logic together
// File: services/app.ts
const MIN_RESPONSES_REQUIRED = 10;

export function validateResponses(count: number) {
  return count >= MIN_RESPONSES_REQUIRED; // Obvious!
}

export function canSubmit(responses: Response[]) {
  return validateResponses(responses.length); // Reuses validation!
}
```

### 4. Reduce Cognitive Load

Minimize what developers need to remember.

```typescript
// HIGH COGNITIVE LOAD: Must remember state machine
let status: 'idle' | 'loading' | 'success' | 'error' = 'idle';
let data: User | null = null;
let error: Error | null = null;

// Developer must remember:
// - When status is 'success', data is not null
// - When status is 'error', error is not null
// - When status is 'loading', both are null
// - Must manually sync these three variables
```

```typescript
// LOW COGNITIVE LOAD: Type system enforces correctness
type AsyncState<T> =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'success'; data: T }
  | { status: 'error'; error: Error };

// Impossible to have invalid state!
// TypeScript enforces that 'success' always has data
// No need to remember the rules - compiler checks them
```

---

## Red Flags: Non-Obvious Code

### Extensive Documentation Required

If you need extensive comments to explain code, the design may be wrong.

```typescript
// RED FLAG: Needs extensive documentation
/**
 * IMPORTANT: You must call initializeSession() before calling this function.
 * IMPORTANT: You must call validateUser() after this function.
 * IMPORTANT: Do not call this function twice for the same user.
 * IMPORTANT: This function modifies global state in SessionManager.
 * IMPORTANT: This function assumes the database connection is open.
 */
function setupUserSession(userId: string, token: string): void {
  // If you need this much documentation, the design is wrong
}
```

```typescript
// OBVIOUS: Self-documenting
class SessionManager {
  async createSession(userId: string): Promise<Session> {
    // All steps handled internally
    // No hidden requirements
    // Returns explicit result
    // No global state modification
  }
}
```

### Frequent Bugs After "Simple" Changes

If small changes often break things, there are unknown unknowns.

```typescript
// RED FLAG: Fragile, breaks easily
// Changing the order of these calls breaks everything!
initDatabase();
loadConfig();
setupAuth();
startServer();
// But nothing in the code tells you the order matters
```

```typescript
// OBVIOUS: Dependencies are explicit
async function startApplication() {
  const config = await loadConfig();
  const db = await initDatabase(config.dbUrl);
  const auth = await setupAuth(db, config.authSecret);
  const server = await startServer(auth, config.port);
  return server;
}
// Order is obvious from the data flow
```

---

## Obviousness for Different Audiences

### Code is More Obvious to Writers Than Readers

**Important**: If you write code and it seems simple to you, but others find it complex, **it is complex**.

```typescript
// Writer thinks: "This is elegant!"
const result = items.reduce((acc, item) => 
  ({...acc, [item.id]: [...(acc[item.id] || []), item]}), 
  {} as Record<string, Item[]>
);

// Reader thinks: "What does this do?"
```

```typescript
// Both writer and reader think: "This is clear"
const itemsById: Record<string, Item[]> = {};
for (const item of items) {
  if (!itemsById[item.id]) {
    itemsById[item.id] = [];
  }
  itemsById[item.id].push(item);
}
```

### Optimize for Readers

Most code is read far more often than it's written.

**Principle**: It's better for writers to suffer than readers.

```typescript
// Easy to write, hard to read
const x = (a, b, c) => a ? b(c) : c;

// Harder to write, easy to read
function applyTransformIfNeeded(
  shouldTransform: boolean,
  transform: (value: string) => string,
  value: string
): string {
  if (shouldTransform) {
    return transform(value);
  }
  return value;
}
```

---

## Making Existing Code More Obvious

### Step 1: Identify Non-Obvious Code

Ask:
- Do I need to read the implementation to use this?
- Are there hidden dependencies?
- Could this break if I change something seemingly unrelated?
- Do I need to remember special rules?

### Step 2: Make Dependencies Explicit

```typescript
// BEFORE: Hidden dependency
function calculateScore() {
  const config = globalConfig; // Where did this come from?
}

// AFTER: Explicit dependency
function calculateScore(config: ScoringConfig) {
  // Now it's obvious what's needed
}
```

### Step 3: Use Type System

```typescript
// BEFORE: Must remember rules
function processUser(user: User, isAdmin: boolean, canEdit: boolean) {
  // Must remember: if isAdmin, canEdit should be true
  // Nothing enforces this
}

// AFTER: Type system enforces rules
type UserPermissions =
  | { role: 'admin'; canEdit: true }
  | { role: 'user'; canEdit: boolean };

function processUser(user: User, permissions: UserPermissions) {
  // Impossible to have admin with canEdit: false
}
```

### Step 4: Consistent Patterns

```typescript
// BEFORE: Inconsistent
async function getUserById(id: string) { }
async function fetchPost(postId: string) { }
async function retrieveComment(id: string) { }

// AFTER: Consistent
async function getUser(id: string) { }
async function getPost(id: string) { }
async function getComment(id: string) { }
```

---

## Checklist

Before merging code:

- [ ] Can a new developer understand this without asking questions?
- [ ] Are all dependencies explicit (not hidden globals)?
- [ ] Is naming consistent with the rest of the codebase?
- [ ] Does the type system prevent invalid states?
- [ ] Is related code colocated?
- [ ] Would I understand this code in 6 months?
- [ ] Can I change this without breaking unrelated code?

---

## Related Rules

- `deep-modules.md` - Simple interfaces hide complexity
- `boolean-hell.md` - Use discriminated unions for obvious state
- `typescript-patterns.md` - Type system enforces correctness
- `DRY.md` - Single source of truth

---

**Source**: "A Philosophy of Software Design" by John Ousterhout, Chapters 2-3
