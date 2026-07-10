---
category: tools
scope: [workflow]
applies-to: [all]
---

# Environment Configuration Rules

**Purpose**: Ensure consistent, safe environment variable management.

---

## Single Source of Truth

**Rule**: All credentials go in `.env.all` (or your designated master env file). No exceptions.

```
BAD: Adding credentials to .env.local
BAD: Hardcoding URLs in application code
BAD: Duplicating credentials in multiple files

GOOD: Add to .env.all in the appropriate section
GOOD: Use environment-specific suffixes (_PROD, _STAGE)
```

---

## File Responsibilities

| File | Who Edits | Purpose |
|------|-----------|---------|
| `.env.all` | Developer | **THE source of truth** |
| `.env.example` | Developer | Documentation template |
| `.env.local` | **System** | Auto-generated - never edit |
| `.env.test` | Developer | Test-specific overrides |

---

## Adding New Variables

### Step 1: Add to `.env.all`

Place in the correct section:

```bash
# ============================================
# SECTION: STRIPE - PAYMENT PROCESSING
# ============================================

# (Add new Stripe variable here)
STRIPE_NEW_FEATURE_KEY=value
```

### Step 2: Add to `.env.example`

Add with placeholder and documentation:

```bash
# New Feature Key (required for feature X)
# STRIPE_NEW_FEATURE_KEY=<your-key>
```

### Step 3: Add to Test Fixtures

Update test fixtures with the new variable:

```typescript
// In REQUIRED_VARIABLES array
{
  name: "STRIPE_NEW_FEATURE_KEY",
  description: "New feature key",
  requiredFor: ["staging", "production"],
}

// In MOCK_VALID_ENV_ALL object
STRIPE_NEW_FEATURE_KEY: "test_value",
```

### Step 4: Update Spec (if required variable)

Update the environment configuration spec with the new variable.

---

## Variable Naming

### Required Prefixes

| Prefix | Meaning | Example |
|--------|---------|---------|
| `VITE_` | Exposed to browser | `VITE_SITE_URL` |
| `DATABASE_` | Database credentials | `DATABASE_URL_PROD` |
| `STRIPE_` | Payment credentials | `STRIPE_SK` |

### Suffixes

| Suffix | Meaning |
|--------|---------|
| `_PROD` | Production value |
| `_STAGE` | Staging value |
| `_KEY` | API key or secret |
| `_URL` | Endpoint URL |
| `_ID` | Identifier |

---

## Validation Before Deploy

### Always Run Before Production

```bash
npm run env:validate:prod
```

This checks:
- All production variables exist
- Stripe keys are `pk_live_` and `sk_live_`
- `PAYMENT_MODE=prod`
- No placeholder values

### CI/CD Integration

The validate script runs automatically in GitHub Actions before production deploys.

---

## What NOT To Do

```typescript
// NEVER hardcode credentials
const databaseUrl = "https://example.database.co";

// ALWAYS use environment variables
const databaseUrl = import.meta.env.VITE_DATABASE_URL;
```

```bash
# NEVER commit .env files
git add .env.all  # WRONG!

# ONLY commit .env.example
git add .env.example  # Correct
```

```bash
# NEVER manually edit .env.local
echo "NEW_VAR=value" >> .env.local  # WRONG!

# Add to .env.all and let the script regenerate
echo "NEW_VAR=value" >> .env.all
npm run dev  # Regenerates .env.local
```

---

## Quick Reference

### Check Configuration

```bash
npm run env:validate       # Current environment
npm run env:validate:prod  # Production readiness
```

### Regenerate .env.local

```bash
npm run dev                 # Auto-regenerates
node scripts/sync-env.mjs   # Manual
```
