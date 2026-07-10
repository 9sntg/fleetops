---
rule: rust/error-handling
title: Rust Error Handling
category: rust
scope: [rust]
priority: recommended
applies-to: [rust]
tags: [error-handling, thiserror, anyhow, result, axum]
---

# Rust Error Handling

**Enforcement**: `cargo clippy` + code review

---

## Core Principle

> "Make illegal states unrepresentable." -- Yaron Minsky

Every fallible operation returns `Result<T, E>`. Panics are reserved for unrecoverable programmer errors. Error types are precise, serializable for HTTP responses, and never leak secrets.

---

## Application Error Enum

Define a single error type for the backend. Use `thiserror` for ergonomic `Display` and `From` implementations:

```rust
// src/error.rs

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("External API error: {0}")]
    ExternalApi(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("Database error: {0}")]
    Database(String),
}
```

### Why one enum?

- Server handlers and client libraries return the same error type
- `#[from]` enables `?` propagation from library errors
- Each variant maps cleanly to a domain boundary
- HTTP status code variants enable clean axum response mapping

---

## Result<T, AppError> for All Internal Functions

```rust
// ✅ GOOD: Typed result alias
pub type AppResult<T> = Result<T, AppError>;

// Service functions use the alias
pub async fn fetch_logs(client: &reqwest::Client) -> AppResult<Vec<LogEntry>> {
    let response = client
        .get("https://api.example.com/graphql")
        .send()
        .await?; // reqwest::Error auto-converts via #[from]

    let body: ApiResponse = response.json().await?;
    Ok(body.into_log_entries())
}
```

```rust
// ❌ BAD: Using Box<dyn Error> in internal functions
pub async fn fetch_logs(
    client: &reqwest::Client,
) -> Result<Vec<LogEntry>, Box<dyn std::error::Error>> {
    // Loses type information, harder to match on specific errors
}
```

---

## The ? Operator for Propagation

Use `?` to propagate errors up the call stack. This works when the error type implements `From<SourceError>` for the function's return error type.

```rust
// ✅ GOOD: ? chains cleanly
pub async fn fetch_events(client: &reqwest::Client, api_key: &str) -> AppResult<Vec<Event>> {
    let response = client
        .get("https://api.example.com/v1/events")
        .bearer_auth(api_key)
        .send()
        .await?;                    // reqwest::Error -> AppError::Http

    let body: ListResponse = response
        .json()
        .await?;                    // reqwest::Error -> AppError::Http

    Ok(body.data)
}
```

```rust
// ❌ BAD: Manual matching when ? would suffice
pub async fn fetch_events(client: &reqwest::Client, api_key: &str) -> AppResult<Vec<Event>> {
    let response = match client.get("https://api.example.com/v1/events").send().await {
        Ok(r) => r,
        Err(e) => return Err(AppError::Http(e)),
    };
    // Verbose and obscures the happy path
}
```

---

## Error Context with .map_err()

When `?` alone does not carry enough context, use `.map_err()` to enrich the error:

```rust
// ✅ GOOD: Adding domain context
pub async fn fetch_issues(client: &reqwest::Client, org: &str, project: &str) -> AppResult<Vec<Issue>> {
    let url = format!("https://api.example.com/projects/{org}/{project}/issues/");

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::ExternalApi(format!("Failed to reach API: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::ExternalApi(
            format!("API returned {status}: {body}")
        ));
    }

    let issues: Vec<Issue> = response
        .json()
        .await
        .map_err(|e| AppError::ExternalApi(format!("Failed to parse response: {e}")))?;

    Ok(issues)
}
```

```rust
// ❌ BAD: Losing the original context
let issues: Vec<Issue> = response.json().await?;
// If this fails, you get "HTTP error: expected value at line 1 column 1"
// with no indication which API response was malformed
```

---

## HTTP Error Response Mapping (axum)

Map error enum variants to HTTP status codes using axum's `IntoResponse` trait:

```rust
// ✅ GOOD: Map error variants to HTTP status codes
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, kind) = match &self {
            AppError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AppError::Config(_) => (StatusCode::INTERNAL_SERVER_ERROR, "config"),
            AppError::ExternalApi(_) => (StatusCode::BAD_GATEWAY, "external_api"),
            AppError::Http(_) => (StatusCode::BAD_GATEWAY, "http"),
            AppError::Serialization(_) => (StatusCode::INTERNAL_SERVER_ERROR, "serialization"),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
            AppError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "database"),
        };

        let body = serde_json::json!({
            "error": {
                "kind": kind,
                "message": self.to_string(),
            }
        });

        (status, Json(body)).into_response()
    }
}
```

This allows axum handlers to return `Result<T, AppError>` directly:

```rust
async fn list_events(
    State(state): State<AppState>,
    Query(params): Query<EventQuery>,
) -> Result<Json<Vec<Event>>, AppError> {
    let events = state.event_service.list(&params).await?;
    Ok(Json(events))
}
```

## Client Library Error Types

A client library (SDK) wraps `reqwest` errors and server error responses into the application error type:

```rust
// ✅ GOOD: Client wraps HTTP errors into domain errors
impl ApiClient {
    async fn handle_response<T: DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T, AppError> {
        let status = response.status();
        if status.is_success() {
            return response.json().await.map_err(|e| {
                AppError::Serialization(serde_json::Error::custom(e.to_string()))
            });
        }

        match status.as_u16() {
            401 => Err(AppError::Unauthorized("Invalid or expired token".into())),
            404 => Err(AppError::NotFound("Resource not found".into())),
            400 => {
                let body = response.text().await.unwrap_or_default();
                Err(AppError::BadRequest(body))
            }
            _ => {
                let body = response.text().await.unwrap_or_default();
                Err(AppError::Internal(format!("Server returned {status}: {body}")))
            }
        }
    }
}
```

---

## anyhow for App-Level / main.rs

Use `anyhow` only at the application boundary (main.rs, setup functions) where you don't need to match on specific error variants:

```rust
// ✅ GOOD: anyhow in main.rs for setup errors
use anyhow::Context;

fn main() -> anyhow::Result<()> {
    let config = config::load()
        .context("Failed to load application configuration")?;

    // ... application setup ...

    Ok(())
}
```

```rust
// ❌ BAD: anyhow in service or command functions
pub async fn fetch_logs(client: &reqwest::Client) -> anyhow::Result<Vec<LogEntry>> {
    // Callers cannot match on specific error variants
    // IPC commands cannot return anyhow::Error (not Serialize)
}
```

**Rule**: `anyhow` at the edges, typed `AppError` everywhere else.

---

## Never Expose API Keys in Error Messages

```rust
// ❌ BAD: API key leaked in error message
let response = client
    .get(&url)
    .header("Authorization", &format!("Bearer {api_key}"))
    .send()
    .await
    .map_err(|e| AppError::ExternalApi(
        format!("Request to {url} with key {api_key} failed: {e}")
    ))?;

// ✅ GOOD: Redacted
let response = client
    .get(&url)
    .header("Authorization", &format!("Bearer {api_key}"))
    .send()
    .await
    .map_err(|e| AppError::ExternalApi(
        format!("Request to API failed: {e}")
    ))?;
```

### DON'T: Log full config structs

```rust
// ❌ BAD
tracing::debug!("Config loaded: {:?}", config);
// Could print: Config { api_token: "sk-live-abc123...", ... }

// ✅ GOOD
tracing::debug!(
    api_configured = !config.api_token.is_empty(),
    db_configured = !config.db_url.is_empty(),
    "Config loaded"
);
```

---

## Error Handling Patterns

### DON'T: Use unwrap() in production code

```rust
// ❌ BAD: Panics at runtime
let config: Config = serde_json::from_str(&raw).unwrap();

// ✅ GOOD: Propagate with context
let config: Config = serde_json::from_str(&raw)
    .map_err(|e| AppError::Config(format!("Invalid config JSON: {e}")))?;
```

### DON'T: Use expect() without a helpful message

```rust
// ❌ BAD: Unhelpful panic message
let port = config.port.expect("missing");

// ✅ GOOD: Descriptive reason (still only for truly impossible states)
let port = config.port.expect("port must be set after validation in load()");
```

### DO: Match on specific error variants when recovery is possible

```rust
// ✅ GOOD: Retry on transient HTTP errors
match services::fetch_logs(&state.http_client).await {
    Ok(logs) => Ok(logs),
    Err(AppError::Http(e)) if e.is_timeout() => {
        tracing::warn!("API timeout, retrying...");
        services::fetch_logs(&state.http_client).await
    }
    Err(e) => Err(e),
}
```

---

## Checklist

Before merging, verify:

- [ ] All fallible functions return `Result<T, AppError>`, not `Option` or panics
- [ ] `?` is used for error propagation instead of manual `match` / `if let Err`
- [ ] `.map_err()` is used to add domain context where `#[from]` is too generic
- [ ] Error type implements `IntoResponse` for axum HTTP responses
- [ ] HTTP status code variants (401, 404, 400, 500) are mapped to error enum variants
- [ ] Client library wraps `reqwest` errors into typed error variants
- [ ] No API keys, tokens, or secrets appear in error messages or logs
- [ ] `unwrap()` does not appear in non-test code (use `?` or `expect()` with a message)
- [ ] `anyhow` is only used in `main.rs` and setup code, not in services or commands
- [ ] HTTP error responses from APIs are checked (`response.status().is_success()`) before parsing JSON
- [ ] Error variants are specific enough to allow callers to take different recovery actions
