---
rule: rust/ratatui-architecture
title: Ratatui TUI Architecture
category: rust
scope: [rust, tui]
priority: required
applies-to: [rust]
tags: [ratatui, crossterm, tui, rendering, event-loop, testbackend]
---

# Ratatui TUI Architecture

**Enforcement**: code review + `TestBackend` snapshot tests on every view.

---

## Core Principle

> Rendering is a pure function of state. The event loop mutates state; `view` only reads it. Never do I/O while drawing.

Ratatui is **immediate-mode**: the entire UI is rebuilt from `App` state on every frame. That is only fast and correct if `view` is pure and cheap.

---

## Separation of concerns (the three seams)

Keep these in separate modules — they are tested differently:

| Module | Owns | Purity | Tested with |
|---|---|---|---|
| `tui/model.rs` | `App` state + `update(msg)` transitions | pure state machine | plain unit tests |
| `tui/view.rs` | `fn view(f: &mut Frame, app: &App)` | **pure**, read-only over `App` | `TestBackend` snapshot |
| `tui/keys.rs` | key/event → `Msg` mapping | pure | table-driven unit tests |

`view` takes `&App` (never `&mut`), performs **no I/O, no `.await`, no blocking, no allocation-heavy work**, and returns nothing but drawn widgets. If you feel the urge to fetch/compute inside `view`, that computation belongs in `update` and its result belongs in `App`.

## The event loop is the only place with I/O

One `tokio::select!` loop owns the terminal and drives everything:

```rust
loop {
    terminal.draw(|f| view(f, &app))?;          // pure render of current state
    tokio::select! {
        Some(ev) = events.next() => {            // crossterm EventStream
            if let Some(msg) = keys::map(ev) { app.update(msg); }
        }
        Some(probe) = probes.recv() => app.update(Msg::Probe(probe)),  // async results arrive as Msgs
        Some(line)  = logs.recv()   => app.update(Msg::Log(line)),
        _ = tick.tick() => app.update(Msg::Tick),
        else => break,
    }
    if app.should_quit { break; }
}
```

- **Async work never runs on the UI task.** Health probes, `ss`/`systemctl` calls, and `journalctl` tails run as spawned tasks that send their results back over channels; the loop folds them in as `Msg`s. See [[async-tokio]].
- **Redraw is driven by messages**, plus a modest tick (e.g. the ~1.5s poll). Do not busy-redraw at max FPS.

## Terminal lifecycle is RAII

Enter/leave raw mode + alternate screen through a guard whose `Drop` always restores the terminal — even on panic or `?` early-return. A panic that leaves the terminal in raw mode is a P0 UX bug. Install a panic hook that restores the terminal before printing the panic.

## Rendering rules

- Precompute display strings/rows in `update`, store them on `App`; `view` just lays them out.
- Constrain layout with `Layout`/`Constraint`; don't hardcode cell math that breaks on resize.
- Keep a bounded ring buffer for logs (see [[async-tokio]]); never grow an unbounded `Vec` you then render.
- Colour = a pure function of state (one `state → Style` map), so status colours are consistent and testable.

## Testing views

Every view has a `TestBackend` test that renders into a fixed-size buffer and asserts on the content:

```rust
let backend = TestBackend::new(80, 24);
let mut terminal = Terminal::new(backend)?;
terminal.draw(|f| view(f, &app_fixture()))?;
assert_snapshot!(terminal.backend()); // insta, or assert on Buffer cells
```

Because `view` is pure, these tests are deterministic and fast. A view you "can't test without a real terminal" is a view doing too much — push the logic back into `update`.
