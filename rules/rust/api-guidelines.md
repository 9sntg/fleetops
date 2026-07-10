---
rule: rust/api-guidelines
title: Rust API Guidelines
category: rust
scope: [rust]
priority: recommended
applies-to: [rust]
tags: [api, naming, documentation, serde, traits]
---

# Rust API Guidelines

**Enforcement**: `cargo clippy` + `cargo doc --no-deps` + code review

---

## Core Principle

> "Rust APIs should be hard to misuse." -- Rust API Guidelines (rust-lang.github.io/api-guidelines)

Public APIs should guide callers toward correct usage through types, naming, and documentation. Every type that crosses an IPC or serialization boundary must be serializable and well-documented.

---

## Naming Conventions

### Conversion methods: as_ / to_ / into_

| Prefix | Cost | Ownership | Example |
|--------|------|-----------|---------|
| `as_`  | Free (borrow/cast) | Borrows `&self` | `as_str()`, `as_bytes()` |
| `to_`  | Expensive (clone/allocate) | Borrows `&self` | `to_string()`, `to_vec()` |
| `into_` | Free (move/reinterpret) | Consumes `self` | `into_inner()`, `into_vec()` |

```rust
// ✅ GOOD: Follows the as_/to_/into_ convention
impl LogEntry {
    /// Returns the message as a string slice (free, borrows).
    pub fn as_message(&self) -> &str {
        &self.message
    }

    /// Converts to a summary string (allocates a new String).
    pub fn to_summary(&self) -> String {
        format!("[{}] {}: {}", self.timestamp, self.source, self.message)
    }

    /// Consumes the entry and returns the owned message (no allocation).
    pub fn into_message(self) -> String {
        self.message
    }
}
```

```rust
// ❌ BAD: Wrong prefix for the cost
impl LogEntry {
    fn to_message(&self) -> &str {  // Should be as_message (it borrows, not clones)
        &self.message
    }

    fn as_summary(&self) -> String {  // Should be to_summary (it allocates)
        format!("[{}] {}", self.timestamp, self.message)
    }
}
```

### Getter methods: no get_ prefix

```rust
// ✅ GOOD: Rust convention omits "get_"
impl AppConfig {
    pub fn api_token(&self) -> &str { &self.api_token }
    pub fn poll_interval(&self) -> Duration { self.poll_interval }
    pub fn is_configured(&self) -> bool { !self.api_token.is_empty() }
}

// ❌ BAD: Java-style getter prefix
impl AppConfig {
    pub fn get_api_token(&self) -> &str { &self.api_token }
    pub fn get_poll_interval(&self) -> Duration { self.poll_interval }
}
```

---

## Standard Trait Implementations

### Debug on all types

Every type should derive or implement `Debug`. This is essential for logging and error messages.

```rust
// ✅ GOOD: Debug on all types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub message: String,
    pub source: LogSource,
    pub level: LogLevel,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogSource {
    Database,
    Api,
    Queue,
}
```

```rust
// ❌ BAD: Missing Debug -- can't use {:?} in logs
pub struct LogEntry {
    pub message: String,
    pub source: LogSource,
}
```

### Display on error types

Error types must implement `Display` (usually via `thiserror`):

```rust
// ✅ GOOD: Display via thiserror
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("API error: {0}")]
    Api(String),
}
```

### Clone and PartialEq where appropriate

```rust
// ✅ GOOD: Clone for types that need to be shared, PartialEq for testability
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub event_type: String,
    pub created: i64,
}

// DON'T derive Clone on types that hold expensive resources
// e.g., reqwest::Client already implements Clone (it's an Arc internally)
pub struct AppState {
    pub http_client: reqwest::Client,  // Clone is cheap (Arc)
    pub config: AppConfig,             // Clone only if small
}
```

### Serialize/Deserialize for IPC types

Every type crossing an IPC or API boundary must derive both:

```rust
// ✅ GOOD: IPC-ready types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardData {
    pub logs: Vec<LogEntry>,
    pub events: Vec<Event>,
    pub issues: Vec<Issue>,
    pub last_updated: String,
}
```

---

## Documentation

### /// doc comments on all pub items

```rust
// ✅ GOOD: Documented public API
/// A single log entry from one of the monitored services.
///
/// # Examples
///
/// ```
/// let entry = LogEntry {
///     message: "Deploy succeeded".into(),
///     source: LogSource::Api,
///     level: LogLevel::Info,
///     timestamp: chrono::Utc::now(),
/// };
/// assert_eq!(entry.source, LogSource::Api);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// The log message content.
    pub message: String,
    /// Which API source produced this entry.
    pub source: LogSource,
    /// Severity level.
    pub level: LogLevel,
    /// When the event occurred (UTC).
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
```

### # Errors section for fallible functions

```rust
/// Fetches recent log entries from an external API.
///
/// # Errors
///
/// Returns `AppError::Api` if the API returns a non-success status.
/// Returns `AppError::Http` if the network request fails.
/// Returns `AppError::Serialization` if the response body cannot be parsed.
pub async fn fetch_logs(
    client: &reqwest::Client,
    token: &str,
) -> Result<Vec<LogEntry>, AppError> {
    // ...
}
```

### # Panics section when applicable

```rust
/// Creates a new AppState from the given configuration.
///
/// # Panics
///
/// Panics if the HTTP client builder fails, which should only happen
/// if TLS initialization fails on the system.
pub fn new(config: AppConfig) -> Self {
    // ...
}
```

---

## #[non_exhaustive] on Public Enums

Adding `#[non_exhaustive]` to public enums allows you to add new variants in future versions without breaking downstream code:

```rust
// ✅ GOOD: Future-proof public enum
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum LogSource {
    Database,
    Api,
    Queue,
}

// Callers must include a wildcard arm:
match source {
    LogSource::Database => { /* ... */ }
    LogSource::Api => { /* ... */ }
    LogSource::Queue => { /* ... */ }
    _ => { /* handle future variants */ }
}
```

```rust
// ❌ BAD: Adding a variant later breaks all match statements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogSource {
    Database,
    Api,
    Queue,
    // Adding Metrics here would break every existing match
}
```

---

## serde rename_all = "camelCase" for JavaScript Interop

Rust uses `snake_case` for fields. JavaScript/TypeScript uses `camelCase`. Bridge the gap at the serialization boundary:

```rust
// ✅ GOOD: camelCase for frontend consumption
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    pub deploy_id: String,        // serializes as "deployId"
    pub service_name: String,     // serializes as "serviceName"
    pub created_at: String,       // serializes as "createdAt"
    pub is_successful: bool,      // serializes as "isSuccessful"
}
```

```rust
// ❌ BAD: Frontend receives snake_case, needs manual conversion
#[derive(Debug, Serialize, Deserialize)]
pub struct Deployment {
    pub deploy_id: String,        // serializes as "deploy_id" -- not idiomatic JS
    pub service_name: String,
}
```

For API responses that arrive in a different format, use `#[serde(rename_all)]` on deserialization and a separate outgoing type for IPC:

```rust
// Incoming: match the external API's format
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ExternalApiEvent {
    pub event_type: String,       // API sends "event_type"
    pub created: i64,
}

// Outgoing: camelCase for the frontend
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEvent {
    pub event_type: String,       // Frontend receives "eventType"
    pub created: i64,
}
```

---

## IPC Type Requirements

All types returned from IPC command functions (e.g., `#[tauri::command]`) must satisfy:

1. `Serialize` (for the success value)
2. `Serialize` on the error type (for the error response)
3. `Debug` (for logging and diagnostics)

```rust
// ✅ GOOD: Complete IPC-ready type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub id: String,
    pub title: String,
    pub culprit: String,
    pub short_id: String,
    pub first_seen: String,
    pub last_seen: String,
    pub event_count: u64,
}
```

### DON'T: Return non-serializable types from commands

```rust
// ❌ BAD: chrono::DateTime does not serialize to a JS-friendly format by default
#[derive(Serialize)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>, // might serialize as RFC3339 object
}

// ✅ GOOD: Use a String for the timestamp, formatted consistently
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub timestamp: String, // "2025-01-15T10:30:00Z"
}

// Or use serde's with attribute
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    #[serde(serialize_with = "serialize_datetime")]
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
```

---

## Checklist

Before merging, verify:

- [ ] Conversion methods use the correct prefix: `as_` (borrow), `to_` (clone), `into_` (consume)
- [ ] Getter methods do not use a `get_` prefix
- [ ] All public types derive `Debug`
- [ ] Error types implement `Display` (via `thiserror` or manually)
- [ ] All IPC types derive both `Serialize` and `Deserialize`
- [ ] IPC types use `#[serde(rename_all = "camelCase")]`
- [ ] Public enums use `#[non_exhaustive]` to allow future extension
- [ ] All `pub` functions and types have `///` doc comments
- [ ] Fallible functions document errors in a `# Errors` section
- [ ] `Clone` and `PartialEq` are derived only where semantically meaningful, not reflexively
