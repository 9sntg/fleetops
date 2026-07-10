---
rule: rust/async-tokio
title: Rust Async & Tokio
category: rust
scope: [rust]
priority: recommended
applies-to: [rust]
tags: [async, tokio, concurrency, wasm, reqwest, axum]
---

# Rust Async & Tokio

**Enforcement**: `cargo clippy` + code review

---

## Core Principle

> "The key insight is that async Rust is not about making things concurrent -- it's about making things composable." -- Alice Ryhl, Tokio maintainer

All I/O-bound work (HTTP calls, database queries, SSE streams) should be async. CPU-bound work should be offloaded with `spawn_blocking`. Never block the async runtime.

---

## Tokio Runtime

Each binary manages its own runtime. Servers use `#[tokio::main]` directly. Desktop apps (e.g., Tauri) have their runtime managed by the framework.

```rust
// ✅ GOOD: Server uses #[tokio::main]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::load()?;
    let pool = PgPool::connect(&config.database_url).await?;
    let app = create_router(pool);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

```rust
// ✅ GOOD: Tauri desktop app -- Tauri manages the runtime
fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![...])
        .run(tauri::generate_context!())
        .expect("failed to run app");
}
```

## axum Handler Patterns

Server route handlers are async functions that receive extractors and return responses.

```rust
// ✅ GOOD: Async axum handler
use axum::{extract::{State, Query}, Json};

async fn list_events(
    State(state): State<AppState>,
    Query(params): Query<EventQuery>,
) -> Result<Json<Vec<Event>>, AppError> {
    let events = state.event_service.list(&params).await?;
    Ok(Json(events))
}
```

## SSE Stream Implementation

Real-time events can be sent to clients via Server-Sent Events (SSE).

```rust
// ✅ GOOD: SSE endpoint using axum + tokio broadcast
use axum::response::sse::{Event as SseEvent, Sse};
use tokio_stream::wrappers::BroadcastStream;
use futures::stream::Stream;

async fn event_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<SseEvent, axum::Error>>> {
    let rx = state.event_bus.subscribe();
    let stream = BroadcastStream::new(rx).map(|result| {
        match result {
            Ok(event) => Ok(SseEvent::default()
                .json_data(&event)
                .unwrap()),
            Err(_) => Ok(SseEvent::default().comment("missed events")),
        }
    });

    Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
}
```

---

## Shared reqwest::Client via App State

Create the HTTP client **once** at startup and share it through managed state. `reqwest::Client` uses connection pooling internally -- creating a new client per request wastes connections and TLS handshakes.

```rust
// ✅ GOOD: Single shared client
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
            .map_err(|e| AppError::Config(format!("Failed to build HTTP client: {e}")))?;

        Ok(Self { http_client, config })
    }
}
```

```rust
// ❌ BAD: Creating a new client per request
async fn get_logs() -> Result<Vec<LogEntry>, AppError> {
    let client = reqwest::Client::new(); // new TLS session, no pooling
    services::fetch_logs(&client).await
}
```

---

## tokio::join! for Parallel API Calls

When you need data from multiple external APIs simultaneously, use `tokio::join!` to run all requests concurrently:

```rust
// ✅ GOOD: Parallel fetching with tokio::join!
async fn get_dashboard(
    state: &AppState,
) -> Result<DashboardData, AppError> {
    let client = &state.http_client;
    let config = &state.config;

    let (logs_result, events_result, issues_result) = tokio::join!(
        services::fetch_logs(client, &config.logs_token),
        services::fetch_events(client, &config.events_key),
        services::fetch_issues(client, &config.issues_token),
    );

    Ok(DashboardData {
        logs: logs_result?,
        events: events_result?,
        issues: issues_result?,
    })
}
```

```rust
// ❌ BAD: Sequential fetching -- 3x slower
async fn get_dashboard(state: &AppState) -> Result<DashboardData, AppError> {
    let logs = services::fetch_logs(&state.http_client).await?;
    let events = services::fetch_events(&state.http_client).await?;
    let issues = services::fetch_issues(&state.http_client).await?;
    // Each waits for the previous to finish
    Ok(DashboardData { logs, events, issues })
}
```

### When one failure should not cancel the others

Use `tokio::join!` and handle each result independently:

```rust
// ✅ GOOD: Partial success is acceptable
let (logs, events, issues) = tokio::join!(
    services::fetch_logs(client, token),
    services::fetch_events(client, key),
    services::fetch_issues(client, token),
);

Ok(DashboardData {
    logs: logs.unwrap_or_default(),
    events: events.unwrap_or_default(),
    issues: issues.unwrap_or_default(),
    errors: collect_errors(&[
        logs.as_ref().err(),
        events.as_ref().err(),
        issues.as_ref().err(),
    ]),
})
```

---

## tokio::select! for Cancellation and Timeouts

Use `select!` when you need to race a future against a cancellation signal or a timeout:

```rust
use tokio::time::{timeout, Duration};

// ✅ GOOD: Timeout on the application level
async fn fetch_with_timeout(
    client: &reqwest::Client,
    url: &str,
    max_wait: Duration,
) -> AppResult<String> {
    match timeout(max_wait, client.get(url).send()).await {
        Ok(Ok(response)) => Ok(response.text().await?),
        Ok(Err(e)) => Err(AppError::Http(e)),
        Err(_elapsed) => Err(AppError::Api(
            format!("Request to {url} timed out after {max_wait:?}")
        )),
    }
}
```

```rust
// ✅ GOOD: select! for cancellation
use tokio::sync::watch;

async fn poll_logs(
    client: &reqwest::Client,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(e) = services::fetch_logs(client).await {
                    tracing::warn!("Polling failed: {e}");
                }
            }
            _ = shutdown.changed() => {
                tracing::info!("Polling stopped by shutdown signal");
                break;
            }
        }
    }
}
```

---

## Timeouts on All HTTP Requests

Every HTTP request must have a timeout. Configure it at the client level and optionally override per-request:

```rust
// ✅ GOOD: Client-level timeout (default for all requests)
let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(30))
    .connect_timeout(Duration::from_secs(5))
    .build()?;

// ✅ GOOD: Per-request timeout override for slow endpoints
let response = client
    .post("https://api.example.com/graphql")
    .timeout(Duration::from_secs(60)) // GraphQL can be slower
    .json(&query)
    .send()
    .await?;
```

```rust
// ❌ BAD: No timeout -- request can hang forever
let client = reqwest::Client::new();
let response = client.get(url).send().await?;
```

---

## DON'T: Hold MutexGuard Across .await

A `std::sync::MutexGuard` is not `Send`. Holding it across an `.await` point will cause a compile error (or deadlock with `tokio::sync::Mutex` if tasks run on the same thread).

```rust
// ❌ BAD: MutexGuard held across await
async fn update_cache(state: &AppState) {
    let mut cache = state.cache.lock().unwrap();
    let fresh_data = fetch_data().await; // guard held across await!
    cache.insert("key", fresh_data);
}

// ✅ GOOD: Drop the guard before awaiting
async fn update_cache(state: &AppState) {
    let fresh_data = fetch_data().await; // await first
    let mut cache = state.cache.lock().unwrap();
    cache.insert("key", fresh_data);
    // guard dropped here
}

// ✅ GOOD: Use a block to scope the guard
async fn update_and_read(state: &AppState) -> String {
    let current = {
        let cache = state.cache.lock().unwrap();
        cache.get("key").cloned()
    }; // guard dropped at end of block

    let fresh = fetch_data().await;

    let mut cache = state.cache.lock().unwrap();
    cache.insert("key", fresh.clone());
    fresh
}
```

If you truly need an async-aware mutex, use `tokio::sync::Mutex` -- but prefer restructuring to avoid it.

---

## DON'T: Block the Async Runtime

Blocking calls (CPU-heavy work, synchronous I/O, `std::thread::sleep`) on the Tokio runtime starve other tasks. Offload with `tokio::task::spawn_blocking`.

```rust
// ❌ BAD: CPU-bound JSON parsing blocks the runtime
async fn parse_large_export(raw: String) -> Result<Vec<LogEntry>, AppError> {
    let entries: Vec<LogEntry> = serde_json::from_str(&raw)?; // blocks if raw is huge
    Ok(entries)
}

// ✅ GOOD: Offload to blocking thread pool
async fn parse_large_export(raw: String) -> Result<Vec<LogEntry>, AppError> {
    let entries = tokio::task::spawn_blocking(move || {
        serde_json::from_str::<Vec<LogEntry>>(&raw)
    })
    .await
    .map_err(|e| AppError::Config(format!("Task panicked: {e}")))?
    .map_err(AppError::Serialization)?;

    Ok(entries)
}
```

```rust
// ❌ BAD: std::thread::sleep in async context
async fn retry_with_delay() {
    std::thread::sleep(Duration::from_secs(1)); // blocks entire thread
}

// ✅ GOOD: tokio::time::sleep is async-aware
async fn retry_with_delay() {
    tokio::time::sleep(Duration::from_secs(1)).await; // yields to runtime
}
```

---

## Background Polling with Graceful Shutdown

Use a background Tokio task with an interval timer for periodic work:

```rust
// ✅ GOOD: Background polling task with graceful shutdown
use tokio::sync::{watch, broadcast};
use tokio::time::{interval, Duration};

pub fn start_polling(
    event_tx: broadcast::Sender<Event>,
    connectors: Vec<Box<dyn Connector>>,
    shutdown_rx: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(30));
        let mut shutdown = shutdown_rx;

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    for connector in &connectors {
                        match connector.fetch_events().await {
                            Ok(events) => {
                                for event in events {
                                    let _ = event_tx.send(event);
                                }
                            }
                            Err(e) => {
                                tracing::warn!(connector = %connector.name(), "Polling error: {e}");
                            }
                        }
                    }
                }
                _ = shutdown.changed() => {
                    tracing::info!("Polling shutdown");
                    break;
                }
            }
        }
    });
}
```

---

## WASM / Cloudflare Workers Async

When targeting WASM (e.g., Cloudflare Workers with `workers-rs`), the async model differs significantly from Tokio.

### Runtime Differences

- **No Tokio runtime** -- `workers-rs` provides its own async runtime
- Use `wasm_bindgen_futures::spawn_local` instead of `tokio::spawn`
- No `tokio::time`, `tokio::fs`, or other Tokio utilities

### HTTP Clients in WASM

```rust
# Cargo.toml
[dependencies]
reqwest = { version = "0.12", default-features = false, features = ["json"] }
```

- Use `reqwest` with `--no-default-features` to avoid pulling in tokio/native-tls
- Enable only the `json` feature for WASM compatibility
- Set timeouts on all requests

### Database Connections

- Use `tokio-postgres` with the JS feature flag for WASM compat
- Get a connection per request (no pooling inside the Worker)
- External services like Hyperdrive handle connection pooling

### Streaming in WASM

```rust
use async_stream::stream;

// SSE streaming in a Worker
let s = stream! {
    for event in events {
        yield format!("data: {}\n\n", serde_json::to_string(&event).unwrap());
    }
};

Response::from_stream(s)
```

- Use `async_stream::stream!` for SSE
- `Response::from_stream()` for streaming responses
- Handle backpressure appropriately

### WASM Async Best Practices

- Keep async blocks small
- Avoid blocking operations (there is no `spawn_blocking` in WASM)
- Use `?` for error propagation
- Test async code with mock services

---

## Checklist

Before merging, verify:

- [ ] Server uses `#[tokio::main]`; desktop apps let the framework manage the runtime
- [ ] `reqwest::Client` is created once in `AppState` and shared, not created per-request
- [ ] Independent API calls use `tokio::join!` for parallelism
- [ ] All HTTP requests have a timeout (client-level or per-request)
- [ ] No `std::sync::MutexGuard` held across `.await` points
- [ ] No blocking calls (`std::thread::sleep`, heavy CPU, synchronous file I/O) on the async runtime
- [ ] Background polling tasks have a shutdown mechanism (`watch` channel or cancellation token)
- [ ] `tokio::task::spawn_blocking` is used for CPU-intensive operations
- [ ] `tokio::time::sleep` is used instead of `std::thread::sleep` in async code
- [ ] Error handling in background tasks logs errors instead of silently swallowing them
- [ ] WASM targets use `reqwest` with `default-features = false` and avoid Tokio dependencies
