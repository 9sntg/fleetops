---
rule: clean-code/component-size
title: Component Size Guidelines
category: clean-code
scope: [general]
priority: recommended
applies-to: [typescript, javascript, react]
tags: [components, SRP, refactoring, LOC, react]
---

# Component Size Guidelines

**Target**: ~100-150 LOC (Lines of Code) per component

**Why**: Single Responsibility Principle, readability, maintainability

---

## The Rule

When a component approaches **~100 LOC**, pause and evaluate:
- Can I extract subcomponents?
- Can I extract custom hooks?
- Am I doing too much in one place?

At **150+ LOC**, evaluate whether the component has multiple responsibilities. If splitting would reduce clarity, keep it together. Optimize for comprehension, not line count.

---

## How to Measure

```bash
# Count lines in a component
wc -l src/features/dashboard/components/AttemptCard.tsx
```

**What counts**:
- Imports: YES (they indicate complexity)
- Comments: YES (part of maintainability)
- Blank lines: YES (readability counts)
- JSX: YES (main content)

---

## Refactoring Strategies

### 1. Extract Subcomponents

**Before** (240 LOC):
```tsx
export default function DashboardHub() {
  // 30 lines of state/hooks

  return (
    <div>
      {/* 40 lines of attempt card rendering */}
      {vm.attempts.map(vmA => (
        <Card key={vmA.role}>
          {/* nested JSX */}
        </Card>
      ))}

      {/* 50 lines of header rendering */}
      <header>
        {/* complex header */}
      </header>

      {/* ... more components ... */}
    </div>
  );
}
```

**After** (150 LOC):
```tsx
// AttemptCardsGrid.tsx (92 LOC)
export function AttemptCardsGrid({ vmAttempts }: Props) {
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

// HeaderSection.tsx (50 LOC)
export function HeaderSection({ title, subtitle }: Props) {
  return (
    <header>
      <h1>{title}</h1>
      <p>{subtitle}</p>
    </header>
  );
}

// DashboardHub.tsx (now 150 LOC)
export default function DashboardHub() {
  // State/hooks

  return (
    <div>
      <HeaderSection title={...} subtitle={...} />
      <AttemptCardsGrid vmAttempts={vm.attempts} />
    </div>
  );
}
```

---

### 2. Extract Custom Hooks

**Before** (Component with hook logic):
```tsx
export function UserProfile() {
  // 30 lines of data fetching logic
  const [user, setUser] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);

  useEffect(() => {
    // Complex fetch logic
  }, []);

  // 50 lines of render logic
  return <div>{/* ... */}</div>;
}
```

**After** (Extract hook):
```tsx
// hooks/useUser.ts
export function useUser(userId: string) {
  const [user, setUser] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);

  useEffect(() => {
    // Complex fetch logic
  }, [userId]);

  return { user, loading, error };
}

// UserProfile.tsx (now smaller)
export function UserProfile() {
  const { user, loading, error } = useUser(userId);

  if (loading) return <Spinner />;
  if (error) return <Error message={error} />;

  return <div>{/* ... */}</div>;
}
```

---

### 3. Split by Concern

If a component handles multiple concerns, split them:

```tsx
// BAD: One component doing everything
export function UserDashboard() {
  // User profile logic
  // Notifications logic
  // Recent activity logic
  // Settings logic
  // Total: 300+ LOC
}

// GOOD: Split by concern
export function UserDashboard() {
  return (
    <>
      <UserProfile />        {/* 80 LOC */}
      <Notifications />      {/* 60 LOC */}
      <RecentActivity />     {/* 90 LOC */}
      <UserSettings />       {/* 70 LOC */}
    </>
  );
}
```

---

## When NOT to Split

**Don't split if**:
- Component is already focused (one clear responsibility)
- Splitting creates unnecessary indirection
- Logic is tightly coupled and can't be separated
- You're just moving lines around without improving clarity
- **Readers must flip between files to understand the logic**

### Red Flag: Splitting That Hurts Understanding

**Bad Split**: If understanding the child requires understanding the parent's context, they should probably stay together.

```tsx
// BAD SPLIT: Child is tightly coupled to parent
// File: QuestionDisplay.tsx
function QuestionDisplay({ question, index }: Props) {
  // Assumes parent has set up specific context
  // Assumes parent handles state updates
  // Can't be understood or used independently
  return <div>{question.text}</div>;
}

// File: Questionnaire.tsx
function Questionnaire() {
  // Complex state management
  // Specific context setup
  // Must read both files to understand how they work together
  return <QuestionDisplay question={q} index={i} />;
}
```

**Good Split**: Child is self-contained and could be used by other parents.

```tsx
// GOOD SPLIT: Child is independent
// File: QuestionCard.tsx
function QuestionCard({ 
  question: string, 
  onAnswer: (value: number) => void 
}: Props) {
  // Self-contained, obvious behavior
  // Could be used by any parent
  // No hidden dependencies
  return (
    <div>
      <p>{question}</p>
      <RatingButtons onSelect={onAnswer} />
    </div>
  );
}

// File: Questionnaire.tsx
function Questionnaire() {
  return <QuestionCard question={q.text} onAnswer={handleAnswer} />;
}
```

**Guideline**: If you find yourself constantly switching between parent and child files to understand how they work together, that's a sign the split may be wrong.

### Example of Bad Split

```tsx
// BAD: Artificial split
function UserCardHeader({ name }: Props) {
  return <h3>{name}</h3>; // 1 line - too small!
}

function UserCardBody({ email }: Props) {
  return <p>{email}</p>; // 1 line - too small!
}

// Better: Keep together
function UserCard({ name, email }: Props) {
  return (
    <div>
      <h3>{name}</h3>
      <p>{email}</p>
    </div>
  );
}
```

---

## Checklist

Before merging code:

- [ ] All components under 150 LOC?
- [ ] Components approaching 100 LOC evaluated for extraction?
- [ ] Extracted components have clear single responsibility?
- [ ] Custom hooks extracted where appropriate?
- [ ] No artificial splits (components have substance)?

---

## Quick Reference

| LOC Range | Action |
|-----------|--------|
| **0-100** | Ideal, no action needed |
| **100-150** | Evaluate for extraction |
| **150-200** | Refactor soon |
| **200+** | Refactor now (blocks SRP) |
