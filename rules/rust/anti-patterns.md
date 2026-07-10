---
rule: rust/anti-patterns
title: Rust Anti-Patterns
category: rust
scope: [rust]
priority: recommended
applies-to: [rust]
tags: [anti-patterns, clippy, code-quality, best-practices]
---

# Rust Anti-Patterns

**Enforcement**: `cargo clippy` + code review

These are patterns to AVOID in Rust codebases. Each anti-pattern includes a bad example, a good alternative, and an explanation of why it matters.

---

## unwrap() in Production Code

### DON'T: Use unwrap() where failure is possible

```rust
// ❌ BAD: Panics if the config file is missing or malformed
let config: AppConfig = serde_json::from_str(&raw).unwrap();

// ❌ BAD: Panics if the lock is poisoned
let cache = state.cache.lock().unwrap();
```

```rust
// ✅ GOOD: Propagate with ? and context
let config: AppConfig = serde_json::from_str(&raw)
    .map_err(|e| AppError::Config(format!("Invalid config: {e}")))?;

// ✅ GOOD: expect() with a reason for truly impossible states
let cache = state.cache.lock().expect("cache mutex poisoned — unrecoverable");
```

**WHY**: `unwrap()` panics at runtime with no context. In a long-running application, a panic crashes the process silently. Use `?` for recoverable errors and `expect("reason")` only when you can prove the error is logically impossible.

---

## Stringly-Typed APIs

### DON'T: Use raw strings where enums belong

```rust
// ❌ BAD: Typos are silent bugs, no exhaustive matching
fn filter_logs(logs: &[LogEntry], source: &str) -> Vec<&LogEntry> {
    logs.iter().filter(|l| l.source == source).collect()
}

// Caller can pass anything:
filter_logs(&logs, "railwya"); // typo compiles fine, returns nothing
```

```rust
// ✅ GOOD: Enum enforces valid values at compile time
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSource {
    Database,
    Api,
    Queue,
}

fn filter_logs(logs: &[LogEntry], source: LogSource) -> Vec<&LogEntry> {
    logs.iter().filter(|l| l.source == source).collect()
}

// Caller:
filter_logs(&logs, LogSource::Api); // typo is a compile error
```

**WHY**: Enums give you exhaustive `match`, IDE autocompletion, and zero-cost type checking. Strings give you runtime surprises.

---

## clone() to Satisfy the Borrow Checker

### DON'T: Clone reflexively to make the compiler happy

```rust
// ❌ BAD: Cloning the entire config every time
fn build_request(state: &AppState) -> reqwest::Request {
    let config = state.config.clone(); // 1KB clone on every request
    let url = format!("{}/logs", config.api_url);
    // ...
}
```

```rust
// ✅ GOOD: Borrow only what you need
fn build_request(api_url: &str, token: &str) -> reqwest::RequestBuilder {
    reqwest::Client::new()
        .get(format!("{api_url}/logs"))
        .bearer_auth(token)
}
```

**WHY**: Unnecessary clones waste memory and CPU. More importantly, they hide design problems. If you need to clone to satisfy the borrow checker, the function probably takes too broad a parameter. Narrow the input.

**Exception**: Cloning data freshly deserialized from JSON is fine (see [ownership-borrowing.md](./ownership-borrowing.md)).

---

## God Structs

### DON'T: Put everything in one massive AppState

```rust
// ❌ BAD: God struct with 15+ fields
pub struct AppState {
    pub http_client: reqwest::Client,
    pub db_token: String,
    pub db_project_id: String,
    pub db_environment_id: String,
    pub payments_key: String,
    pub payments_webhook_secret: String,
    pub monitoring_token: String,
    pub monitoring_org: String,
    pub monitoring_project: String,
    pub poll_interval: Duration,
    pub log_level: String,
    pub cache: HashMap<String, Vec<LogEntry>>,
    pub last_fetch: Option<Instant>,
    pub retry_count: u32,
    pub window_position: (i32, i32),
}
```

```rust
// ✅ GOOD: Split by domain
pub struct AppState {
    pub http_client: reqwest::Client,
    pub config: AppConfig,
    pub cache: Arc<Mutex<LogCache>>,
}

pub struct AppConfig {
    pub database: DatabaseConfig,
    pub payments: PaymentsConfig,
    pub monitoring: MonitoringConfig,
    pub polling: PollingConfig,
}

pub struct DatabaseConfig {
    pub token: String,
    pub project_id: String,
    pub environment_id: String,
}

pub struct PaymentsConfig {
    pub api_key: String,
    pub webhook_secret: String,
}

pub struct MonitoringConfig {
    pub token: String,
    pub org: String,
    pub project: String,
}
```

**WHY**: God structs make every function depend on everything. Splitting by domain means each service module only needs its own config, not the entire state. This improves testability and reduces coupling.

---

## Deref Polymorphism

### DON'T: Abuse Deref for inheritance-like behavior

```rust
// ❌ BAD: Using Deref as "inheritance"
use std::ops::Deref;

struct ApiClient {
    inner: reqwest::Client,
    token: String,
}

impl Deref for ApiClient {
    type Target = reqwest::Client;
    fn deref(&self) -> &reqwest::Client {
        &self.inner
    }
}

// Now ApiClient magically has all reqwest::Client methods
// This is confusing and breaks the principle of least surprise
```

```rust
// ✅ GOOD: Explicit delegation or composition
struct ApiClient {
    client: reqwest::Client,
    token: String,
}

impl ApiClient {
    pub async fn fetch_logs(&self) -> Result<Vec<LogEntry>, AppError> {
        self.client
            .post(API_URL)
            .bearer_auth(&self.token)
            .send()
            .await?;
        // ...
    }
}
```

**WHY**: `Deref` is meant for smart pointers (`Box`, `Arc`, `Rc`). Using it for "inheritance" confuses readers and makes the public API unpredictable. Explicit methods are clearer.

---

## Blocking in Async Context

### DON'T: Use synchronous/blocking calls inside async functions

```rust
// ❌ BAD: std::fs::read_to_string blocks the Tokio thread
async fn load_config() -> Result<AppConfig, AppError> {
    let raw = std::fs::read_to_string("config.json")?; // BLOCKS
    let config: AppConfig = serde_json::from_str(&raw)?;
    Ok(config)
}

// ❌ BAD: reqwest::blocking in async context
async fn fetch_data() -> Result<String, AppError> {
    let body = reqwest::blocking::get("https://api.example.com/v1/events")? // BLOCKS
        .text()?;
    Ok(body)
}
```

```rust
// ✅ GOOD: Use async equivalents
async fn load_config() -> Result<AppConfig, AppError> {
    let raw = tokio::fs::read_to_string("config.json").await?;
    let config: AppConfig = serde_json::from_str(&raw)?;
    Ok(config)
}

// ✅ GOOD: Use async reqwest
async fn fetch_data(client: &reqwest::Client) -> Result<String, AppError> {
    let body = client
        .get("https://api.example.com/v1/events")
        .send()
        .await?
        .text()
        .await?;
    Ok(body)
}

// ✅ GOOD: spawn_blocking for unavoidable sync work
async fn parse_large_file(path: String) -> Result<Vec<LogEntry>, AppError> {
    tokio::task::spawn_blocking(move || {
        let raw = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&raw)?)
    })
    .await
    .map_err(|e| AppError::Config(format!("Task failed: {e}")))?
}
```

**WHY**: Tokio uses a small thread pool (default: number of CPU cores). A single blocking call starves all other async tasks on that thread. The UI freezes, polling stops, and HTTP requests time out.

---

## Leaking Implementation Details

### DON'T: Expose concrete types when a trait or type alias would suffice

```rust
// ❌ BAD: Caller is coupled to HashMap and BTreeMap
pub fn get_log_index(logs: &[LogEntry]) -> HashMap<String, BTreeMap<String, Vec<&LogEntry>>> {
    // Changing the internal data structure is a breaking change
}
```

```rust
// ✅ GOOD: Return a purpose-built type
pub struct LogIndex {
    inner: HashMap<String, BTreeMap<String, Vec<LogEntry>>>,
}

impl LogIndex {
    pub fn entries_for_source(&self, source: &str) -> &[LogEntry] {
        // Encapsulated -- internal structure can change freely
    }
}

pub fn build_log_index(logs: Vec<LogEntry>) -> LogIndex {
    // ...
}
```

**WHY**: Returning raw collection types locks you into an implementation. A wrapper type lets you change the internals, add caching, or optimize without breaking callers.

---

## Excessive Arc<Mutex<>>

### DON'T: Wrap everything in Arc<Mutex<>> by default

```rust
// ❌ BAD: Mutex for everything, even read-heavy data
pub struct AppState {
    pub config: Arc<Mutex<AppConfig>>,          // Config rarely changes
    pub http_client: Arc<Mutex<reqwest::Client>>, // Client is already Clone (Arc)
    pub cache: Arc<Mutex<HashMap<String, Vec<LogEntry>>>>,
}
```

```rust
// ✅ GOOD: Right tool for each case
pub struct AppState {
    pub config: AppConfig,              // Immutable after startup -- no lock needed
    pub http_client: reqwest::Client,   // Already internally Arc'd -- Clone is cheap
    pub cache: Arc<RwLock<LogCache>>,   // Read-heavy -- RwLock allows concurrent reads
}
```

**WHY**: `Mutex` serializes all access. For read-heavy data (like a log cache), `RwLock` allows many concurrent readers. For immutable data (like config loaded at startup), no lock is needed at all. `reqwest::Client` is already `Clone` via an internal `Arc`.

---

## Project-Specific Anti-Patterns

### DON'T: Hardcode API URLs

```rust
// ❌ BAD: URL buried in service code
pub async fn fetch_logs(client: &reqwest::Client, token: &str) -> AppResult<Vec<LogEntry>> {
    let response = client
        .post("https://api.example.com/graphql") // hardcoded
        .send()
        .await?;
}

// ✅ GOOD: URL comes from config or constants
const API_URL: &str = "https://api.example.com/graphql";

pub async fn fetch_logs(
    client: &reqwest::Client,
    config: &ApiConfig,
) -> AppResult<Vec<LogEntry>> {
    let response = client
        .post(&config.api_url) // or use the constant
        .bearer_auth(&config.token)
        .send()
        .await?;
}
```

**WHY**: Hardcoded URLs make testing impossible (you can't point at a mock server) and prevent environment-based configuration.

### DON'T: Mix configuration loading with business logic

```rust
// ❌ BAD: Service reads env vars directly
pub async fn fetch_events(client: &reqwest::Client) -> AppResult<Vec<Event>> {
    let api_key = std::env::var("API_KEY")
        .map_err(|_| AppError::Config("API_KEY not set".into()))?;

    client
        .get("https://api.example.com/v1/events")
        .bearer_auth(&api_key)
        .send()
        .await?;
}

// ✅ GOOD: Config is injected, service is pure logic
pub async fn fetch_events(
    client: &reqwest::Client,
    api_key: &str,
) -> AppResult<Vec<Event>> {
    client
        .get("https://api.example.com/v1/events")
        .bearer_auth(api_key)
        .send()
        .await?;
}
```

**WHY**: Services that read env vars directly cannot be tested without modifying the environment. Inject configuration through function parameters or the `AppState`.

---

## Checklist

Before merging, verify:

- [ ] No `unwrap()` in non-test code (use `?` or `expect("reason")`)
- [ ] String parameters that represent a fixed set of values are replaced with enums
- [ ] `clone()` calls have a justifying comment or are on cheap-to-clone types (`Arc`, small structs)
- [ ] `AppState` is split by domain, not a god struct
- [ ] `Deref` is only implemented for smart-pointer-like types
- [ ] No `std::fs`, `std::thread::sleep`, or `reqwest::blocking` inside async functions
- [ ] Public functions return domain types, not raw `HashMap` / `Vec` internals
- [ ] `Arc<Mutex<>>` is not used where `RwLock`, immutability, or cheap `Clone` would suffice
- [ ] API URLs are constants or config values, not inline string literals in service code
- [ ] Services receive configuration via parameters, not by reading env vars directly
