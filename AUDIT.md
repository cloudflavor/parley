# parley — Codebase Audit

Last audited: 2026-05-07

## Overview

| Metric | Value |
|---|---|
| Total source files (Rust) | 34 |
| Total lines (Rust) | ~7,368 |
| Functions | 318 |
| Structs/Enums | 118 |
| Primary language | Rust (87%), JavaScript (12%) |

---

## P1 — Must Fix

### Error Handling

- **No crate-level error type.** Every module invents its own error handling with ad-hoc `anyhow` or `std::io::Error` returns. Create `src/error.rs` with a `thiserror` `Error` enum. This is a library crate — using `anyhow` in public APIs is anti-pattern.
- **.unwrap() in non-test code:**
  - `src/git/review/mod.rs:435`, lines 321-322 — three `.unwrap()` calls in `resolve_review`
  - `src/service.rs:281` — `.unwrap()` in `add_review_comment`
  - `src/service.rs:303,305` — two more `.unwrap()` calls
  - `src/tui/app.rs:420` — `App::start` unwraps
- **.expect() mixing strategies:** `src/config.rs:622` — `Config::new()` returns `Result` but uses `.expect()` internally. Pick one strategy.
- **No error type per domain.** Each `domain/` module should have its own error alias mapping back to the crate-level `Error`.

### Complexity

- **`src/git/review/mod.rs` — 44 functions, 661 lines.** Mixes git review resolution, thread state management, comment manipulation, and diff parsing. Split into: `resolver.rs`, `thread.rs`, `comment.rs`, `status.rs`.
- **`src/service.rs` — single large file, no public types.** Split into `comment_service.rs`, `review_manager.rs`, etc.
- **`update_thread` is a god-function** — `•40` call-site references showing it does too much.

### Testing

- **Only 4 test files**, all in `src/git/review/`. Zero integration tests. Zero doc tests.
- **No snapshot tests** for TUI output or diff rendering.

---

## P2 — Should Fix

### Performance

- **`Arc<RwLock<Config>>`** in `src/config.rs:601` — configs are immutable after init. Use `Arc<Config>` or just copy. `RwLock` adds unnecessary runtime overhead.
- **`.map(|c| c.clone()).collect()`** in `src/services/review_service.rs:355` — use `.cloned()`.
- **`Arc<RwLock<Review>>`** in `src/tui/app.rs` — every event handler clones the Arc. Question whether interior mutability is needed or snapshots suffice.

### Naming

- **`src/service.rs`** should be `src/services/` directory to match other module conventions.
- **`thread_status` vs `thread_state`** — ambiguous, mixed in same module. Unify.
- **`resolve_review` vs `create_review` vs `load_review`** — "review" overloaded. Consider `GitReview`, `CodeReview`, `ReviewThread`.

### Documentation

- **No `#![deny(missing_docs)]`** in `lib.rs`. Enable for public APIs.
- **Missing `///` doc comments** on many `pub` structs and functions.
- **Comments describe *what* not *why*.** Per skill: "structure and naming should replace commentary."

---

## P3 — Nice to Have

### Testing

- Add property-based tests for complex logic (thread anchoring, diff resolution).
- Use `cargo insta` for snapshot testing generated diff output.

### Documentation

- Add `//!` module-level comments explaining each module's purpose.
- Create `ARCHITECTURE.md` documenting domain relationships.

### Naming

- Add `ConfigBuilder` for complex config initialization instead of `Config::new_with_secrets()`.

### Security

- **`src/tui/terminal.rs`** — `Terminal::setup()` doesn't validate terminal capabilities/dimensions before proceeding.
- **No input validation on CLI numeric args** — clap derive doesn't include range constraints.

### Dependencies

- Replace `anyhow` with `thiserror` in public API surface.
- Add `cargo clippy -- -D warnings` to CI.
- Add `cargo audit` for vulnerability checks.

---

## Summary

| Priority | Count | Status |
|---|---|---|
| P1 | 4 | 🔴 Must fix |
| P2 | 9 | 🟡 Should fix |
| P3 | 10 | 🟢 Nice to have |
| **Total** | **23** | |
