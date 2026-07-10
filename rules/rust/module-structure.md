---
rule: rust/module-structure
title: Rust Module Structure
category: rust
scope: [rust]
priority: recommended
applies-to: [rust]
tags: [modules, architecture, workspace, cargo, organization]
---

# Rust Module Structure

**Enforcement**: Code review

---

## Core Principle

> "A well-structured Rust project reads like a table of contents: you know where everything lives before you open a file." -- Tim McNamara

Follow a layered architecture within a Cargo workspace. Split the workspace into focused crates with clear dependency rules.

---

## Workspace Layout

The following is an illustrative multi-crate workspace layout. Adapt the crate names and structure to your project's domain:

```
Cargo.toml               # Workspace root
crates/
├── core/                # Shared types, error types, traits — LEAF crate (no internal deps)
│   └── src/
│       ├── lib.rs
│       ├── error.rs     # AppError enum
│       ├── models/      # Event, Deployment, Status, etc.
│       └── traits.rs    # Connector trait, Store trait
├── connectors/          # API clients for external services (depends on core)
│   └── src/
│       ├── lib.rs
│       ├── service_a.rs
│       ├── service_b.rs
│       └── service_c.rs
├── server/              # axum HTTP server (depends on core + connectors)
│   └── src/
│       ├── main.rs
│       ├── handlers/    # Route handlers (thin)
│       ├── state.rs     # AppState (PgPool, event bus, connectors)
│       └── config.rs
├── sdk/                 # Rust client for the server API (depends on core)
│   └── src/
│       ├── lib.rs
│       └── client.rs    # ApiClient (reqwest-based)
└── cli/                 # CLI tool (depends on sdk + core)
    └── src/
        ├── main.rs
        └── commands/    # tail, status, config, etc.
```

### Crate Dependency Rules

```
core             (leaf — no internal deps)
connectors       -> core
server           -> core, connectors
sdk              -> core
cli              -> core, sdk
```

- **core is a leaf**: No crate dependencies within the workspace. Every other crate can depend on core.
- **connectors depends on core**: Uses core types (Event, AppError) but not server or SDK.
- **server depends on core + connectors**: Orchestrates connectors, stores events, serves the API.
- **sdk depends on core only**: HTTP client for the server API, shares types from core.
- **Clients (CLI, desktop app) depend on sdk + core**: Never import connectors or server directly.

---

## Per-Crate Module Layout

Within each crate, follow a layered structure: **handlers** (thin request handlers) call **services** (business logic) which operate on **models** (pure data types).

### Example: server crate

```
crates/server/src/
├── main.rs              # Entry point, axum server
├── lib.rs               # Re-exports, app setup
├── handlers/            # Route handlers (thin)
│   ├── mod.rs
│   ├── events.rs
│   ├── deployments.rs
│   └── health.rs
├── services/            # Business logic
│   ├── mod.rs
│   └── event_service.rs
├── config.rs            # Configuration loading
└── state.rs             # AppState struct
```

### Why this layout?

- **One module per API source**: Each connector gets its own file. Each handler group gets its own file.
- **Layered separation**: Handlers never talk to connectors directly. Services never know about HTTP. Models never contain logic.
- **Flat where possible**: config.rs, state.rs live at the crate root because they are cross-cutting and singular.

---

## Visibility Rules

### Default to pub(crate)

Items should be visible only within the crate unless they need to cross crate boundaries.

```rust
// ✅ GOOD: pub(crate) for internal types
// crates/connectors/src/service_a.rs

pub(crate) async fn fetch_logs(
    client: &reqwest::Client,
    token: &str,
) -> Result<Vec<LogEntry>, AppError> {
    // Only the crate's public API needs to call this
}

// Internal helper -- private to the module
fn parse_response(body: &str) -> Result<Vec<RawLog>, AppError> {
    // ...
}
```

```rust
// ❌ BAD: Everything is pub
pub async fn fetch_logs(...) -> ... { }
pub fn parse_response(...) -> ... { }  // No one outside this module needs this
```

### pub for cross-crate types

Types shared between crates (e.g., in `core`) must be `pub`. Types used only within a single crate should be `pub(crate)`:

```rust
// crates/core/src/models/event.rs

/// A single event, shared across crates via the core crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub message: String,
    pub level: LogLevel,
    pub timestamp: String,
    pub deploy_id: String,
}

/// Internal response shape from an external API. Not exposed outside the connectors crate.
#[derive(Debug, Deserialize)]
pub(crate) struct ApiGraphQLResponse {
    pub data: ApiData,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiData {
    pub logs: Vec<RawLog>,
}
```

---

## Layer Responsibilities

### handlers/ -- Thin HTTP Handlers (server crate)

Handlers are the HTTP surface. They validate input, delegate to a service, and return the result. They contain **no business logic**.

```rust
// ✅ GOOD: Thin handler -- validate, delegate, return
// crates/server/src/handlers/events.rs

use axum::{extract::{State, Query}, Json};
use app_core::{Event, AppError};
use crate::state::AppState;

pub async fn list_events(
    State(state): State<AppState>,
    Query(params): Query<EventQuery>,
) -> Result<Json<Vec<Event>>, AppError> {
    if let Some(ref source) = params.source {
        if source.is_empty() {
            return Err(AppError::BadRequest("source must not be empty".into()));
        }
    }

    let events = state.event_service.list(&params).await?;
    Ok(Json(events))
}
```

```rust
// ❌ BAD: Business logic in a handler
pub async fn list_events(
    State(state): State<AppState>,
) -> Result<Json<Vec<Event>>, AppError> {
    let response = state.http_client
        .post("https://api.example.com/graphql")
        .json(&serde_json::json!({ "query": "{ logs { message } }" }))
        .send()
        .await?;
    // 30 more lines of parsing...
    // This belongs in the connectors crate
}
```

### connectors/ and services/ -- Business Logic and API Clients

Connectors (in the `connectors` crate) contain the API client logic: building requests, parsing responses, applying transformations. Services (in the `server` crate) orchestrate connectors and manage persistence.

```rust
// ✅ GOOD: Connector encapsulates all API interaction
// crates/connectors/src/service_a.rs

use crate::error::AppError;
use crate::models::{LogEntry, ApiGraphQLResponse};

const API_URL: &str = "https://api.example.com/graphql";

const LOGS_QUERY: &str = r#"
    query($first: Int!) {
        deploymentLogs(first: $first) {
            edges {
                node {
                    message
                    severity
                    timestamp
                }
            }
        }
    }
"#;

pub(crate) async fn fetch_logs(
    client: &reqwest::Client,
    token: &str,
) -> Result<Vec<LogEntry>, AppError> {
    let body = serde_json::json!({
        "query": LOGS_QUERY,
        "variables": { "first": 100 }
    });

    let response = client
        .post(API_URL)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::ExternalApi(format!("Network error: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(AppError::ExternalApi(format!("API returned {status}")));
    }

    let graphql: ApiGraphQLResponse = response
        .json()
        .await
        .map_err(|e| AppError::ExternalApi(format!("Parse error: {e}")))?;

    Ok(graphql.into_log_entries())
}
```

### models/ -- Pure Data Types

Models are plain structs and enums. They derive serialization traits and may have simple conversion methods, but no I/O or business logic.

```rust
// ✅ GOOD: Pure data with conversions
// crates/core/src/models/log.rs

use serde::{Deserialize, Serialize};

/// A log entry displayed in the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub message: String,
    pub level: LogLevel,
    pub timestamp: String,
    pub service_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}
```

```rust
// ❌ BAD: Model that makes HTTP calls
impl ApiGraphQLResponse {
    pub async fn fetch(client: &reqwest::Client) -> Result<Self, AppError> {
        // Models should not know about HTTP
    }
}
```

---

## mod.rs Files

Each directory module has a `mod.rs` that declares submodules and re-exports the public surface:

```rust
// connectors/src/lib.rs
pub mod service_a;
pub mod service_b;
pub mod service_c;
```

```rust
// server/src/handlers/mod.rs
pub(crate) mod events;
pub(crate) mod health;
pub(crate) mod config;
```

```rust
// core/src/models/mod.rs
pub mod event;
pub mod severity;
pub mod source;
pub mod common;
```

---

## lib.rs and main.rs

### main.rs -- Minimal Entry Point

```rust
// crates/server/src/main.rs
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    app_server::run().await
}
```

### lib.rs -- App Setup and Wiring

```rust
// crates/server/src/lib.rs
mod config;
mod handlers;
mod state;

pub use state::AppState;

pub async fn run() -> anyhow::Result<()> {
    let config = config::load()?;
    tracing_subscriber::fmt().init();

    let pool = sqlx::PgPool::connect(&config.database_url).await?;
    let app_state = AppState::new(pool, &config)?;
    let app = create_router(app_state);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    tracing::info!(addr = %config.listen_addr, "Server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
```

---

## Common Patterns

### Shared models in core crate

Types used across multiple crates live in `crates/core/src/models/`:

```rust
// crates/core/src/models/common.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiSource {
    ServiceA,
    ServiceB,
    ServiceC,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedLogEntry {
    pub source: ApiSource,
    pub message: String,
    pub level: String,
    pub timestamp: String,
}
```

### state.rs -- AppState struct

```rust
// crates/server/src/state.rs

use crate::config::AppConfig;
use crate::error::AppError;

pub struct AppState {
    pub http_client: reqwest::Client,
    pub config: AppConfig,
}

impl AppState {
    pub fn new(config: AppConfig) -> Result<Self, AppError> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("MyApp/1.0")
            .build()
            .map_err(|e| AppError::Config(format!("HTTP client build error: {e}")))?;

        Ok(Self { http_client, config })
    }
}
```

---

## Checklist

Before merging, verify:

- [ ] Handlers are thin: validate input, delegate to service, return result
- [ ] Connectors contain all external API interaction logic
- [ ] Models are pure data types with no I/O
- [ ] Visibility defaults to `pub(crate)`; only cross-crate types are `pub`
- [ ] Each API source has its own file in the connectors crate
- [ ] Crate dependencies follow: core (leaf) <- connectors <- server; core <- sdk <- clients
- [ ] `mod.rs` files re-export only what other modules need
- [ ] `main.rs` is minimal (calls `lib::run()` or sets up the server/app)
- [ ] No circular dependencies between crates
- [ ] Shared types live in the `core` crate, not duplicated across other crates
- [ ] Internal API response types (e.g., GraphQL shapes) are `pub(crate)`, not `pub`
