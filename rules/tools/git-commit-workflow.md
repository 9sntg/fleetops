---
category: tools
scope: [workflow]
applies-to: [all]
---

# Git Commit Workflow

## When to Commit

After completing **major development work** (2 phases or more), automatically create a Git commit.

## Commit Process

1. **Stage changed files** -- commit only files related to the completed work
2. **Use Conventional Commits** for the commit title:
   - `feat:` -- new feature
   - `fix:` -- bug fix
   - `refactor:` -- code restructuring
   - `docs:` -- documentation changes
   - `test:` -- adding/updating tests
   - `chore:` -- maintenance tasks
3. **Write descriptive body** -- explain what was changed and why, based on the prompts/tasks that led to these changes

## Rules

- ALWAYS commit after major development milestones
- ALWAYS use conventional commit format for titles
- ALWAYS explain the reasoning in the commit body
- NEVER push automatically
- NEVER push to main branch

## Example

```
feat: add user authentication with JWT tokens

- Added JWT token generation in auth service
- Created login/logout endpoints
- Added middleware for protected routes

Context: Implemented based on authentication spec to support
user sessions across the platform.
```
