---
category: tools
scope: [workflow]
applies-to: [all]
---

# Changelog Workflow

Best practices for maintaining the project changelog (`versions.md`).

---

## Purpose

The changelog tracks all significant changes to the project, providing:
- Clear history of features, fixes, and improvements
- Version documentation for releases
- Context for future developers and AI agents

---

## When to Update

Update `versions.md` after completing:

- **New features** - Any user-facing functionality
- **Bug fixes** - Resolved issues affecting users
- **Refactors** - Significant code restructuring
- **Infrastructure changes** - Build, deployment, tooling updates
- **Documentation** - Major documentation additions

---

## Format

Follow this structure for each version entry:

```markdown
## vX.Y (YYYY-MM-DD)

### Feature/Change Name

Brief description of the main change.

#### Changes
- **Item**: Change description
- **Item**: Change description

#### Files Added
- `path/to/file.tsx` - Description

#### Files Modified
- `path/to/file.tsx` - Change description

---
```

---

## Version Numbering

Use semantic versioning:

- **Major (X.0)** - Breaking changes, major rewrites
- **Minor (0.X)** - New features, significant additions
- **Patch (0.0.X)** - Bug fixes, small improvements (optional)

---

## Rules

- **ALWAYS** update `versions.md` at the end of significant work
- **ALWAYS** include the date in version headers
- **ALWAYS** list files added/modified for traceability
- **ALWAYS** keep versions in descending order (newest first)
- **NEVER** remove or modify historical entries
- **NEVER** skip version updates for significant changes

---

## Adding a New Version Entry

1. Open `versions.md` in project root
2. Add new version section at the TOP (after `# Changelog` header)
3. Follow the format template above
4. Include all relevant changes, files added, and files modified
5. Add horizontal rule (`---`) to separate from previous version

---

## Example Entry

```markdown
## v0.4 (2026-01-22)

### Shopping Cart Improvements

Enhanced cart functionality with better UX.

#### Changes
- **Quantity Picker**: Added +/- buttons for easier quantity adjustment
- **Stock Validation**: Real-time stock checking before add to cart
- **Empty State**: Improved empty cart messaging with CTA

#### Files Added
- `src/components/QuantityPicker.tsx` - Reusable quantity input component

#### Files Modified
- `src/features/cart/components/CartItem.tsx` - Integrated QuantityPicker
- `src/features/cart/hooks/useCart.ts` - Added stock validation

---
```

---

## Integration with Git Commits

After completing work:

1. Run tests and linting (see `linting-workflow.md`)
2. **Update `versions.md`** with changes
3. Create git commit (see `git-commit-workflow.md`)

The changelog update should be part of the same commit as the feature/fix.

---

## Related Rules

- `git-commit-workflow.md` - Commit conventions
- `linting-workflow.md` - Pre-commit checks
