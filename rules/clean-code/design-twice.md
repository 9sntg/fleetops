---
rule: clean-code/design-twice
title: Design It Twice
category: clean-code
scope: [general]
priority: recommended
applies-to: [typescript, javascript, react]
tags: [architecture, decision-making, design, trade-offs, planning]
---

# Design It Twice

**Rule**: Before implementing a major feature, consider at least 2-3 radically different approaches.

**Core Idea**: Your first idea is rarely the best. Exploring alternatives reveals better solutions and trade-offs.

---

## Why Design Twice?

### 1. First Ideas Are Rarely Best

The first approach that comes to mind is usually:
- Based on recent experience (not necessarily applicable)
- Influenced by what's easy to implement (not what's best)
- Limited by current mental model (missing better patterns)

### 2. Comparison Reveals Trade-offs

You can't evaluate an approach in isolation. Only by comparing alternatives do you see:
- What's truly essential vs. incidental complexity
- Which approach is simpler
- Which approach is more flexible
- Which approach is easier to understand

### 3. Learning Compounds

Each alternative you consider teaches you something:
- About the problem domain
- About potential solutions
- About what matters most

**Even if you pick your first idea**, you'll implement it better after considering alternatives.

---

## When to Apply

### Design Twice For:

- **New features** (not trivial bug fixes)
- **New services/modules** (significant architectural decisions)
- **Complex refactoring** (restructuring existing code)
- **Public APIs** (interfaces others will depend on)
- **Data models** (hard to change later)

### Don't Design Twice For:

- **Bug fixes** (just fix the bug)
- **Trivial changes** (renaming a variable)
- **Well-established patterns** (follow existing conventions)
- **Time-critical hotfixes** (ship first, refactor later)

---

## The Process

### Step 1: List 2-3 Approaches

Make them **radically different**, not just variations.

```typescript
// BAD: These are variations, not different approaches
// Approach 1: Use useState
// Approach 2: Use useState with useCallback
// Approach 3: Use useState with useMemo

// GOOD: These are fundamentally different
// Approach 1: Local state with useState
// Approach 2: Global state with Context
// Approach 3: Server state with React Query
```

### Step 2: List Pros/Cons for Each

Be honest about trade-offs.

```markdown
## Approach 1: Local State (useState)
**Pros:**
- Simple, no dependencies
- Easy to understand
- Fast to implement

**Cons:**
- State lost on unmount
- Can't share between components
- Re-renders entire component

## Approach 2: Global State (Context)
**Pros:**
- Shared across components
- Persists across unmounts
- Single source of truth

**Cons:**
- More boilerplate
- All consumers re-render
- Harder to test

## Approach 3: Server State (React Query)
**Pros:**
- Automatic caching
- Background refetching
- Optimistic updates
- Built-in loading/error states

**Cons:**
- External dependency
- Learning curve
- Overkill for simple cases
```

### Step 3: Pick the Simplest That Meets Requirements

**Not** the most clever. **Not** the most feature-rich. **The simplest that works.**

Ask:
- Which approach is easiest to understand?
- Which approach is easiest to change later?
- Which approach has the fewest dependencies?
- Which approach handles the common case best?

---

## Real-World Example: Questionnaire State Management

### The Problem

Need to manage questionnaire state (responses, progress, completion).

### Approach 1: Component State

```typescript
// All state in the component
function Questionnaire() {
  const [responses, setResponses] = useState<Response[]>([]);
  const [currentQuestion, setCurrentQuestion] = useState(0);
  const [isComplete, setIsComplete] = useState(false);

  const saveResponse = (questionId: string, value: number) => {
    setResponses(prev => [...prev, { questionId, value }]);
    setCurrentQuestion(prev => prev + 1);
    if (currentQuestion === TOTAL_QUESTIONS - 1) {
      setIsComplete(true);
    }
  };

  // ... render
}
```

**Pros:**
- Simple, no external dependencies
- Easy to understand
- Fast to implement

**Cons:**
- State lost on unmount
- Hard to test business logic
- Can't share state between components
- Logic mixed with UI

### Approach 2: Custom Hook

```typescript
// Extract state management to hook
function useQuestionnaireState() {
  const [responses, setResponses] = useState<Response[]>([]);
  const [currentQuestion, setCurrentQuestion] = useState(0);

  const saveResponse = useCallback((questionId: string, value: number) => {
    setResponses(prev => [...prev, { questionId, value }]);
    setCurrentQuestion(prev => prev + 1);
  }, []);

  const isComplete = responses.length === TOTAL_QUESTIONS;

  return { responses, currentQuestion, saveResponse, isComplete };
}

// Component uses hook
function Questionnaire() {
  const { responses, currentQuestion, saveResponse, isComplete } = useQuestionnaireState();
  // ... render
}
```

**Pros:**
- Logic separated from UI
- Testable in isolation
- Reusable across components
- Derived state (isComplete) computed

**Cons:**
- Still lost on unmount
- Can't share between distant components

### Approach 3: Service Layer + React Query

```typescript
// Service handles business logic
class QuestionnaireService {
  async saveResponse(attemptId: string, questionId: string, value: number) {
    // Validate, save to DB, return updated state
  }

  async getAttempt(attemptId: string) {
    // Fetch current state from DB
  }
}

// Component uses React Query
function Questionnaire({ attemptId }: Props) {
  const { data: attempt } = useQuery({
    queryKey: ['questionnaire', attemptId],
    queryFn: () => questionnaireService.getAttempt(attemptId)
  });

  const { mutate: saveResponse } = useMutation({
    mutationFn: (data: ResponseData) => 
      questionnaireService.saveResponse(attemptId, data.questionId, data.value),
    onSuccess: () => {
      queryClient.invalidateQueries(['questionnaire', attemptId]);
    }
  });

  // ... render
}
```

**Pros:**
- State persists (in DB)
- Automatic caching
- Optimistic updates
- Background refetching
- Shared across all components
- Business logic in service (testable)

**Cons:**
- More complex
- Requires backend
- Network latency
- More dependencies

### Decision

**Chose Approach 3** because:
1. State must persist (user can leave and return)
2. Multiple components need access (hub, questionnaire, results)
3. Already have a backend
4. React Query handles complexity for us

**But learned from Approach 2**:
- Keep business logic in services
- Use custom hooks for component-specific logic
- Compute derived state instead of storing it

---

## Example: API Design

### The Problem

Design an API for submitting attempts.

### Approach 1: Single Endpoint, Action Parameter

```typescript
// POST /api/questionnaire
{
  "action": "submit",
  "attemptId": "123"
}
```

### Approach 2: RESTful Endpoints

```typescript
// POST /api/questionnaire/attempts/:id/submit
```

### Approach 3: GraphQL Mutation

```graphql
mutation SubmitAttempt($attemptId: ID!) {
  submitAttempt(attemptId: $attemptId) {
    id
    status
    scores
  }
}
```

---

## Checklist

Before implementing a major feature:

- [ ] Listed at least 2 approaches?
- [ ] Made approaches radically different (not just variations)?
- [ ] Listed honest pros/cons for each?
- [ ] Considered ease of understanding?
- [ ] Considered ease of change?
- [ ] Considered dependencies?
- [ ] Picked simplest that meets requirements?
- [ ] Learned something from alternatives?

---

## Common Mistakes

### Mistake 1: Variations, Not Alternatives

```typescript
// These are the same approach with minor tweaks
Approach 1: Use async/await
Approach 2: Use async/await with try/catch
Approach 3: Use async/await with error handling utility
```

**Fix**: Make them fundamentally different.

```typescript
Approach 1: Async/await with try/catch
Approach 2: Promise chains with .catch()
Approach 3: Result type (no exceptions)
```

### Mistake 2: Picking the Most Complex

**Fix**: Pick the **simplest** that meets requirements.

### Mistake 3: Not Actually Considering Alternatives

**Fix**: Genuinely explore each alternative. You might be surprised.

---

## Tips

### 1. Sketch Before Coding

Don't write full implementations. Sketch interfaces:

```typescript
// Approach 1 sketch
interface QuestionnaireState {
  responses: Response[];
  saveResponse(q: string, v: number): void;
}

// Approach 2 sketch
class QuestionnaireService {
  async save(attemptId: string, response: Response): Promise<void>;
  async get(attemptId: string): Promise<Attempt>;
}
```

### 2. Ask Others

Show your approaches to teammates. They might:
- Spot issues you missed
- Suggest better alternatives
- Prefer a different approach

### 3. Consider Future Changes

Ask: "What if we need to...?"
- Support offline mode
- Add real-time collaboration
- Scale to 1M users
- Support mobile app

Which approach handles these best?

### 4. Prototype If Uncertain

If you can't decide, build quick prototypes:
- Spend 30 minutes on each
- Don't make them production-ready
- Just enough to feel the approach

---

## Related Rules

- `obvious-design.md` - Pick the most obvious approach
- `deep-modules.md` - Simple interfaces, rich implementation
- `SOLID.md` - Open/Closed Principle (design for extension)

---

**Source**: "A Philosophy of Software Design" by John Ousterhout, Chapter 11
