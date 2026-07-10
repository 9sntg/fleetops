---
rule: rust/ownership-borrowing
title: Rust Ownership & Borrowing
category: rust
scope: [rust]
priority: recommended
applies-to: [rust]
tags: [ownership, borrowing, lifetimes, memory, borrow-checker]
---

# Rust Ownership & Borrowing

**Enforcement**: `cargo clippy` + code review

---

## Core Principle

> "Ownership is Rust's most unique feature. It enables Rust to make memory safety guarantees without a garbage collector." -- Klabnik & Matsakis, *The Rust Programming Language*

Every value in Rust has exactly one owner. When the owner goes out of scope, the value is dropped. Borrowing lets you reference data without taking ownership.

---

## Ownership Rules

Three rules the compiler enforces at all times:

1. Each value has exactly **one owner**
2. When the owner goes out of scope, the value is **dropped**
3. Ownership can be **moved** (transferred), not implicitly copied (unless `Copy`)

```rust
// ✅ GOOD: Ownership is clear
fn process_logs(logs: Vec<LogEntry>) -> Summary {
    // `logs` is owned here, dropped at end of function
    summarize(&logs)
}

// ❌ BAD: Using a value after it was moved
fn process_logs(logs: Vec<LogEntry>) {
    let archived = logs; // ownership moved to `archived`
    println!("{:?}", logs); // ERROR: value used after move
}
```

---

## Borrowing Rules

Two forms of borrowing, mutually exclusive:

| Borrow type | Syntax | Count allowed | Aliasing | Mutation |
|-------------|--------|---------------|----------|----------|
| Shared      | `&T`   | Many          | Yes      | No       |
| Exclusive   | `&mut T` | One         | No       | Yes      |

**You cannot have `&mut T` while any `&T` exists for the same value.**

```rust
// ✅ GOOD: Multiple shared borrows
fn display_logs(logs: &[LogEntry]) {
    for log in logs {
        println!("{}", log.message);
    }
}

fn count_errors(logs: &[LogEntry]) -> usize {
    logs.iter().filter(|l| l.level == Level::Error).count()
}

fn report(logs: &Vec<LogEntry>) {
    display_logs(logs);   // &logs — shared borrow
    let n = count_errors(logs); // &logs — shared borrow (fine, both are &T)
    println!("Errors: {n}");
}
```

```rust
// ❌ BAD: Shared and exclusive borrow at the same time
fn bad_mutation(logs: &mut Vec<LogEntry>) {
    for log in logs.iter() {     // shared borrow of logs starts here
        if log.is_stale() {
            logs.remove(0);      // ERROR: cannot borrow `*logs` as mutable
        }
    }
}

// ✅ GOOD: Collect indices first, then mutate
fn good_mutation(logs: &mut Vec<LogEntry>) {
    let stale_indices: Vec<usize> = logs
        .iter()
        .enumerate()
        .filter(|(_, l)| l.is_stale())
        .map(|(i, _)| i)
        .collect();

    for i in stale_indices.into_iter().rev() {
        logs.remove(i);
    }
}
```

---

## Lifetime Elision Rules

The compiler applies three rules so you rarely need explicit lifetime annotations:

1. **Each input reference gets its own lifetime**: `fn f(a: &str, b: &str)` becomes `fn f<'a, 'b>(a: &'a str, b: &'b str)`
2. **If there is exactly one input lifetime, it is assigned to all outputs**: `fn f(s: &str) -> &str` becomes `fn f<'a>(s: &'a str) -> &'a str`
3. **If one input is `&self` or `&mut self`, its lifetime is assigned to all outputs**

When these rules are insufficient, you must annotate explicitly:

```rust
// ✅ GOOD: Elision handles this (rule 2)
fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

// ✅ GOOD: Explicit lifetime needed (two input references, no self)
fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() > b.len() { a } else { b }
}
```

---

## Cow<str> for Flexible Ownership

`Cow<'a, str>` (Clone on Write) accepts both `&str` and `String`. It avoids unnecessary allocation when the borrowed form suffices.

```rust
use std::borrow::Cow;

// ✅ GOOD: Accept both borrowed and owned strings
fn normalize_source(name: &str) -> Cow<'_, str> {
    if name.contains(' ') {
        Cow::Owned(name.replace(' ', "_"))
    } else {
        Cow::Borrowed(name) // no allocation
    }
}

// Useful for source labels that are usually static
fn format_source_label(source: &str) -> Cow<'_, str> {
    match source {
        "database" | "api" | "queue" => Cow::Borrowed(source),
        other => Cow::Owned(format!("custom:{other}")),
    }
}
```

---

## Common Borrow Checker Fights and Solutions

### Returning a reference to a local variable

```rust
// ❌ BAD: Reference outlives the local
fn get_default_url() -> &str {
    let url = String::from("https://api.example.com");
    &url // ERROR: returns reference to data owned by function
}

// ✅ GOOD: Return an owned value
fn get_default_url() -> String {
    String::from("https://api.example.com")
}

// ✅ GOOD: Return a static reference if the data is truly static
fn get_default_url() -> &'static str {
    "https://api.example.com"
}
```

### Borrowing self while iterating over a field

```rust
// ❌ BAD: Immutable borrow of self.logs conflicts with mutable method
impl LogStore {
    fn process_all(&mut self) {
        for log in &self.logs {   // immutable borrow of self
            self.archive(log);    // ERROR: cannot borrow `*self` as mutable
        }
    }
}

// ✅ GOOD: Separate the data from the operation
impl LogStore {
    fn process_all(&mut self) {
        let logs: Vec<LogEntry> = self.logs.drain(..).collect();
        for log in logs {
            self.archive(log);
        }
    }
}
```

### Splitting borrows across struct fields

```rust
// ✅ GOOD: Rust allows borrowing different fields simultaneously
struct AppState {
    primary_logs: Vec<LogEntry>,
    secondary_logs: Vec<LogEntry>,
}

fn process(state: &mut AppState) {
    let r = &mut state.primary_logs; // borrows only this field
    let s = &state.secondary_logs;   // borrows only this field -- OK
    merge(r, s);
}
```

---

## Anti-Pattern: clone() to Silence the Borrow Checker

### When clone() masks a design issue

```rust
// ❌ BAD: Cloning to work around borrow conflict -- hides a design problem
fn update_dashboard(&mut self) {
    let config = self.config.clone(); // unnecessary 1KB clone every tick
    self.render(&config);
}

// ✅ GOOD: Restructure so borrow conflict disappears
fn update_dashboard(&mut self) {
    // Split the struct or extract the method to take only what it needs
    let output = render(&self.config); // immutable borrow only
    self.canvas = output;
}
```

### When clone() is perfectly fine

```rust
// ✅ OK: Cloning deserialized API response data
// JSON deserialization already allocated Strings; clone is explicit and expected
async fn fetch_logs(client: &reqwest::Client) -> Result<Vec<LogEntry>, AppError> {
    let response: ApiResponse = client
        .get("https://api.example.com/graphql")
        .send()
        .await?
        .json()
        .await?;

    // Clone from deserialized data is fine -- the Strings are already heap-allocated
    let entries: Vec<LogEntry> = response
        .data
        .logs
        .iter()
        .map(|l| LogEntry {
            message: l.message.clone(),
            source: LogSource::Api,
            timestamp: l.timestamp,
        })
        .collect();

    Ok(entries)
}
```

**Rule of thumb**: `clone()` on freshly deserialized JSON data is acceptable. `clone()` inside a hot loop or to dodge a borrow error you don't understand is a code smell.

---

## Framework-Specific Guidelines

- **API response data**: External API responses are deserialized into owned structs. Cloning fields from these structs is expected and cheap relative to the network call.
- **Tauri command return values**: Must be owned types (`String`, `Vec<T>`, structs). Never try to return `&str` from a `#[tauri::command]`.
- **AppState shared via `tauri::State<'_, AppState>`**: This is a managed reference. Use `Arc<Mutex<T>>` or `Arc<RwLock<T>>` for interior mutability when state must be updated.

```rust
// ✅ GOOD: Tauri command returns owned data
#[tauri::command]
async fn get_logs(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<LogEntry>, AppError> {
    let client = &state.http_client;
    let logs = services::fetch_logs(client).await?;
    Ok(logs) // owned Vec returned across IPC
}
```

---

## Checklist

Before merging, verify:

- [ ] No `clone()` calls added solely to silence borrow checker errors without a comment justifying the clone
- [ ] Functions accept `&str` instead of `String` when they do not need ownership
- [ ] Functions accept `&[T]` instead of `&Vec<T>` for slice parameters
- [ ] No references returned to local variables
- [ ] Lifetime annotations are only added where elision rules are insufficient
- [ ] `Cow<str>` is considered for functions that sometimes allocate and sometimes don't
- [ ] IPC commands return owned types, not references
- [ ] `Arc<Mutex<T>>` / `Arc<RwLock<T>>` is used for shared mutable state in AppState, not `RefCell`
