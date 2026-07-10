---
rule: rust/naming
title: Rust Naming Conventions
category: rust
scope: [rust]
priority: recommended
applies-to: [rust]
tags: [naming, conventions, clippy, style]
---

# Rust Naming Conventions

**Enforcement**: `cargo clippy` (warns on non-idiomatic names) + code review

---

## Core Principle

> "Naming is the most important and most difficult part of programming." -- Rust API Guidelines

Rust has strong, community-wide naming conventions. Following them means every Rust developer can read your code without a style guide detour. Deviating from them signals "this code was written by someone unfamiliar with Rust."

---

## Case Conventions

| Item | Convention | Example |
|------|-----------|---------|
| Functions, methods | `snake_case` | `fetch_logs`, `parse_response` |
| Local variables | `snake_case` | `log_entry`, `api_key` |
| Modules | `snake_case` | `mod database`, `mod payments` |
| Crate names (in code) | `snake_case` | `use my_lib::...` |
| Types (structs, enums) | `PascalCase` | `LogEntry`, `AppError` |
| Traits | `PascalCase` | `Serialize`, `Display` |
| Enum variants | `PascalCase` | `LogSource::Database` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_RETRY_COUNT` |
| Statics | `SCREAMING_SNAKE_CASE` | `DEFAULT_TIMEOUT` |
| Type parameters | Single uppercase letter or short PascalCase | `T`, `E`, `Item` |
| Lifetimes | Short lowercase | `'a`, `'ctx` |

```rust
// ✅ GOOD: Follows all conventions
const MAX_LOG_ENTRIES: usize = 1000;

pub struct LogEntry {
    pub message: String,
    pub source: LogSource,
}

pub enum LogSource {
    Database,
    Api,
    Queue,
}

pub fn fetch_logs(client: &reqwest::Client) -> AppResult<Vec<LogEntry>> {
    let api_url = "https://api.example.com/graphql";
    // ...
}
```

```rust
// ❌ BAD: Mixed conventions
const maxLogEntries: usize = 1000;       // should be SCREAMING_SNAKE_CASE

pub struct log_entry {                    // should be PascalCase
    pub Message: String,                  // should be snake_case
}

pub enum logSource {                      // should be PascalCase
    database,                              // variant should be PascalCase
}

pub fn FetchLogs() -> Vec<log_entry> {  // should be snake_case
}
```

---

## Trait Naming

Traits describe capabilities. Use adjectives or the `-able` / `-er` suffix:

| Pattern | Examples |
|---------|----------|
| Adjective | `Clone`, `Debug`, `Send`, `Sync` |
| -able | `Serialize`, `Deserialize`, `Iterable` |
| -er (for single-method) | `Reader`, `Writer`, `Handler` |
| Noun (for role) | `Iterator`, `Future`, `Stream` |

```rust
// ✅ GOOD: Trait naming
trait Fetchable {
    async fn fetch(&self, client: &reqwest::Client) -> AppResult<Vec<LogEntry>>;
}

trait LogFormatter {
    fn format_entry(&self, entry: &LogEntry) -> String;
}
```

```rust
// ❌ BAD: Verb or vague trait names
trait DoFetch { }          // Verb -- should be Fetchable
trait LogStuff { }         // Vague -- what "stuff"?
trait ILogService { }      // C#/Java-style "I" prefix -- not idiomatic Rust
```

---

## Crate Naming

In `Cargo.toml`, crate names use **kebab-case**. In Rust code, they use **snake_case** (Cargo converts automatically):

```toml
# Cargo.toml
[package]
name = "my-app-lib"

[dependencies]
serde-json = "1.0"
tokio = { version = "1", features = ["full"] }
```

```rust
// In code, hyphens become underscores
use my_app_lib::AppError;
use serde_json::Value;
```

---

## No Stuttering

Avoid repeating the module or crate name in the item name. Callers already write `module::Item`, so `module::ModuleItem` stutters:

```rust
// ❌ BAD: Stuttering
mod app {
    pub struct AppConfig { }     // app::AppConfig
    pub struct AppError { }      // app::AppError
    pub fn app_connect() { }     // app::app_connect
}

mod database {
    pub struct DatabaseClient { }    // database::DatabaseClient
    pub fn database_query() { }      // database::database_query
}
```

```rust
// ✅ GOOD: No stuttering
mod app {
    pub struct Config { }           // app::Config
    pub struct Error { }            // app::Error
    pub fn connect() { }            // app::connect
}

mod database {
    pub struct Client { }           // database::Client
    pub fn query() { }              // database::query
}
```

**Exception**: Top-level types that are widely used across the crate may keep the prefix to avoid ambiguity (e.g., `AppError` in `error.rs` since it's imported everywhere).

---

## Conversion Methods: as_ / to_ / into_

| Prefix | Borrows? | Allocates? | Consumes? | Example |
|--------|----------|------------|-----------|---------|
| `as_`  | Yes (`&self`) | No | No | `as_str()`, `as_bytes()` |
| `to_`  | Yes (`&self`) | Yes | No | `to_string()`, `to_vec()` |
| `into_` | No | Maybe | Yes (`self`) | `into_inner()`, `into_string()` |

```rust
impl LogEntry {
    // ✅ as_ : free reference cast
    pub fn as_message(&self) -> &str {
        &self.message
    }

    // ✅ to_ : allocates a new String
    pub fn to_summary(&self) -> String {
        format!("{} [{}]", self.message, self.source)
    }

    // ✅ into_ : consumes self, returns owned inner data
    pub fn into_parts(self) -> (String, LogSource) {
        (self.message, self.source)
    }
}
```

```rust
// ❌ BAD: Wrong prefix
impl LogEntry {
    pub fn to_message(&self) -> &str { &self.message }   // should be as_ (borrows)
    pub fn as_summary(&self) -> String { format!("...") } // should be to_ (allocates)
    pub fn get_parts(self) -> (String, LogSource) { }     // should be into_ (consumes)
}
```

---

## Predicate Methods: is_ / has_

Boolean-returning methods use `is_` or `has_` prefixes:

```rust
// ✅ GOOD: Predicate naming
impl LogEntry {
    pub fn is_error(&self) -> bool {
        self.level == LogLevel::Error
    }

    pub fn has_stack_trace(&self) -> bool {
        self.message.contains("at ") || self.message.contains("Traceback")
    }
}

impl AppConfig {
    pub fn is_database_configured(&self) -> bool {
        !self.database.connection_string.is_empty()
    }

    pub fn has_all_sources(&self) -> bool {
        self.is_database_configured()
            && self.is_api_configured()
            && self.is_monitoring_configured()
    }
}
```

```rust
// ❌ BAD: Missing predicate prefix
impl LogEntry {
    pub fn error(&self) -> bool { }           // Ambiguous -- sounds like a getter
    pub fn check_stack_trace(&self) -> bool { } // "check" is a verb, not a predicate
}
```

---

## Constructors: new()

The primary constructor is `new()`. Secondary constructors use `with_` or descriptive names:

```rust
// ✅ GOOD: Constructor naming
impl AppConfig {
    /// Creates a new config by reading environment variables.
    pub fn new() -> Result<Self, AppError> {
        // Primary constructor
    }

    /// Creates a config from a TOML file path.
    pub fn from_file(path: &Path) -> Result<Self, AppError> {
        // Named constructor for alternative input
    }

    /// Creates a config with explicit values (useful for testing).
    pub fn with_tokens(
        db_token: String,
        api_key: String,
        monitoring_token: String,
    ) -> Self {
        // Builder-like secondary constructor
    }
}
```

```rust
// ❌ BAD: Non-standard constructor names
impl AppConfig {
    pub fn create() -> Self { }       // Use new()
    pub fn make() -> Self { }         // Use new()
    pub fn initialize() -> Self { }   // Use new()
    pub fn build() -> Self { }        // build() is for the Builder pattern only
}
```

---

## Example Naming Table

A reference table for consistent naming across your project:

| Concept | Type name | Variable name | Module |
|---------|-----------|---------------|--------|
| Log entry | `LogEntry` | `log_entry` | `models::log` |
| External event | `Event` | `event` | `models::event` |
| App configuration | `AppConfig` | `config` | `config` |
| Central error type | `AppError` | `err`, `error` | `error` |
| HTTP client wrapper | `AppState` | `state` | `state` |
| API service | (free functions) | -- | `services::api_name` |
| Log severity | `LogLevel` | `level` | `models::common` |
| API source enum | `ApiSource` | `source` | `models::common` |
| Polling interval | `POLL_INTERVAL_SECS` | `poll_interval` | `config` |
| Dashboard aggregate | `DashboardData` | `dashboard` | `models::common` |
| Result alias | `AppResult<T>` | -- | `error` |

### API naming pattern

Server HTTP handlers follow `verb_noun` in snake_case. Client libraries and TypeScript wrappers use `camelCase`:

```rust
// Server handler (snake_case)
pub async fn list_events(Query(params): Query<EventQuery>) -> Result<Json<EventResponse>, AppError> { }
pub async fn get_health() -> Json<HealthStatus> { }
```

```rust
// SDK client (snake_case)
let events = client.query_events(&filters).await?;
let health = client.get_health().await?;
```

```typescript
// TypeScript SDK wrapper (camelCase)
const events = await sdk.queryEvents(filters);
const health = await sdk.getHealth();
```

---

## Checklist

Before merging, verify:

- [ ] All functions, methods, and variables use `snake_case`
- [ ] All types, traits, and enum variants use `PascalCase`
- [ ] All constants and statics use `SCREAMING_SNAKE_CASE`
- [ ] Crate name in `Cargo.toml` uses `kebab-case`
- [ ] No `get_` prefix on getter methods
- [ ] Conversion methods use the correct `as_` / `to_` / `into_` prefix
- [ ] Boolean methods use `is_` or `has_` prefix
- [ ] Primary constructors are named `new()`
- [ ] No stuttering (e.g., `database::DatabaseClient` should be `database::Client`)
- [ ] Trait names are adjectives, `-able` forms, or `-er` forms (not verbs or "I-" prefixed)
