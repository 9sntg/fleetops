---
rule: clean-code/SOLID
title: SOLID Principles for React + TypeScript
category: clean-code
scope: [general]
priority: recommended
applies-to: [typescript, javascript, react]
tags: [SOLID, SRP, OCP, LSP, ISP, DIP, design-patterns, architecture]
---

# SOLID Principles for React + TypeScript

**Purpose**: Maintainable, extensible code that survives refactoring.

---

## The Five Principles

### S - Single Responsibility Principle

**Rule**: One reason to change per function/class/component/hook.

```typescript
// BAD: Component does too much
export default function DashboardHub() {
  // 30 lines of state/hooks
  // 40 lines of attempt card rendering
  // 50 lines of header rendering
  // 100+ lines of other logic
  // Total: 295 LOC - TOO BIG!
}

// GOOD: Extracted to focused components
// AttemptCardsGrid.tsx (92 LOC)
export function AttemptCardsGrid({ vmAttempts, attempts }: Props) {
  return (
    <>
      {vmAttempts.map(vmA => (
        <Card key={vmA.role}>
          <AttemptCard {...vmA} />
        </Card>
      ))}
    </>
  );
}

// DashboardHub.tsx (now 150 LOC)
export default function DashboardHub() {
  return (
    <div>
      <HeaderSection />
      <AttemptCardsGrid vmAttempts={vm.attempts} />
    </div>
  );
}
```

**Target**: ~100-150 LOC per component

#### Cognitive Load Consideration

Sometimes **more lines of code = less complexity**, if it reduces what developers need to remember.

```typescript
// CLEVER BUT HIGH COGNITIVE LOAD
// Developer must understand: reduce, spread, nullish coalescing, type assertion
const result = items.reduce((a, b) => 
  ({...a, [b.id]: (a[b.id] || 0) + b.value}), 
  {} as Record<string, number>
);

// MORE LINES, BUT OBVIOUS
// Each step is clear, no mental gymnastics required
const result: Record<string, number> = {};
for (const item of items) {
  if (!result[item.id]) {
    result[item.id] = 0;
  }
  result[item.id] += item.value;
}
```

**Principle**: Optimize for **readability**, not brevity. Code is read far more often than written.

---

### O - Open/Closed Principle

**Rule**: Open to extend, closed to modify.

```typescript
// BAD: Must modify registry for new actions
function handleAction(action: string) {
  if (action === 'init') return handleInit();
  if (action === 'save') return handleSave();
  if (action === 'submit') return handleSubmit();
  // Add new action -> MUST MODIFY THIS FUNCTION
}

// GOOD: Strategy pattern
class ActionHandlerRegistry {
  register(action: string, handler: ActionHandler) { ... }
  get(action: string): ActionHandler | undefined { ... }
}

// Easy to add new handlers WITHOUT modifying existing code
registry.register(ACTION_INIT, new InitHandler());
registry.register(ACTION_SAVE, new SaveResponseHandler());
registry.register(ACTION_SUBMIT, new SubmitHandler());
// Add new handler here - NO MODIFICATION needed
```

---

### L - Liskov Substitution Principle

**Rule**: Honor contracts. Use discriminated unions.

```typescript
// GOOD: Type-safe discriminated unions
type Status =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'done'; count: number };

function renderStatus(s: Status) {
  switch (s.kind) {
    case 'idle':    return '--';
    case 'loading': return 'Loading...';
    case 'done':    return String(s.count);
    default: {
      const _exhaustive: never = s; // TS error if case missing
      return _exhaustive;
    }
  }
}
```

**Benefit**: TypeScript enforces all cases handled!

---

### I - Interface Segregation Principle

**Rule**: Many small, focused interfaces > one "god" interface.

```typescript
// BAD: God interface
interface UserCardProps {
  user: User;
  showAvatar: boolean;
  showEmail: boolean;
  showPhone: boolean;
  showAddress: boolean;
  showActions: boolean;
  canEdit: boolean;
  canDelete: boolean;
  onEdit?: () => void;
  onDelete?: () => void;
  // ... 20+ props
}

// GOOD: Focused interfaces
interface UserCardProps {
  user: User;
  variant: 'minimal' | 'standard' | 'detailed';
  actions?: UserActions;
}

interface UserActions {
  onEdit?: () => void;
  onDelete?: () => void;
}
```

---

### D - Dependency Inversion Principle

**Rule**: Depend on abstractions. Pass dependencies in.

```typescript
// BAD: Hardcoded global dependency
import { supabase } from '@app/integrations/supabase/client';

export class UserService {
  async getUser(id: string) {
    return supabase.from('users').select().eq('id', id); // HARDCODED!
  }
}

// GOOD: Dependency injection
export class UserService {
  constructor(private db: SupabaseClient) {} // INJECTED

  async getUser(id: string) {
    return this.db.from('users').select().eq('id', id);
  }
}

// Usage via DI container
const userService = useUserService(); // Container provides dependency
```

---

## Practical Checklist

Before merging code, verify:

- [ ] **S**: Each component has one clear responsibility?
- [ ] **O**: Can extend without modifying existing code?
- [ ] **L**: Contracts honored, discriminated unions used?
- [ ] **I**: Props focused, not sprawling?
- [ ] **D**: Dependencies injected, not hardcoded?

---

## Related Rules

- `component-size.md` - SRP component sizing
- `typescript-patterns.md` - Discriminated unions and exhaustive switches
- `deep-modules.md` - Simple interfaces, rich implementation
