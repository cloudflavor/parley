# Parley Refactoring TODO

> Generated from comprehensive code review following Rust best practices: no unwraps/expects, use anyhow, async tokio everywhere, modern CLI parsing.

## Priority 1: Safety - Remove unwrap/expect from Production Code

### 1.1 `src/services/ai_session.rs:821`
**Issue**: `status.expect("status is present when not timed out")`
**Location**: `invoke_provider()` function, after timeout check
**Fix**: Replace with proper error handling using `context()` or `with_context()`
**Impact**: HIGH - Production code path, can panic if status is None
**Status**: ✅ **COMPLETED**

### 1.2 `src/tui/app/state.rs:1519`
**Issue**: `self.ai_task.take().expect("checked as some")`
**Location**: `poll_ai_task()` method
**Fix**: Use `if let Some(task) = self.ai_task.take()` pattern instead
**Impact**: HIGH - Production code path in TUI event loop
**Status**: ✅ **COMPLETED**

### 1.3 `src/tui/app/input.rs:811`
**Issue**: `anchors.last().expect("anchors checked as non-empty")`
**Location**: Thread navigation logic
**Fix**: Add defensive check, return early if anchors is empty
**Impact**: MEDIUM - Production code path in TUI input handling
**Status**: ✅ **COMPLETED**

### 1.4 `src/tui/app/helpers.rs:66`
**Issue**: `OffsetDateTime::from_unix_timestamp_nanos().expect()`
**Location**: `format_timestamp_utc()` function
**Fix**: Return error string or use fallback formatting on invalid timestamps
**Impact**: MEDIUM - Production code path used throughout TUI rendering
**Status**: ✅ **COMPLETED**

### 1.5 `src/services/ai_session.rs:637-643`
**Issue**: `prompt_template()` uses `unwrap_or_else(|| panic!(...))`
**Location**: Embedded prompt file access
**Fix**: Return `Result<&str>` or use `anyhow::bail!` for missing templates
**Impact**: MEDIUM - Production code path, but embedded files should always exist
**Status**: ✅ **COMPLETED**

## Priority 2: Code Quality - Consolidate and Improve

### 2.1 Duplicate `now_ms()` Functions
**Files**:
- `src/services/ai_session.rs:1109`
- `src/services/review_service.rs:241`
- `src/tui/app/state/anchor.rs:pub(crate) fn now_ms_utc()`

**Fix**: Create shared utility module `src/utils/time.rs` with single implementation
**Benefit**: DRY principle, easier maintenance
**Status**: ⏳ **PENDING** - Still 3 duplicate implementations exist

### 2.2 CLI Argument Parsing Error Types
**File**: `src/cli/args.rs`
**Issue**: Custom `FromStr` implementations return `String` errors instead of proper error types
**Fix**: Use `thiserror` to create `CliArgError` enum, or return `anyhow::Error`
**Benefit**: Better error messages, consistent error handling
**Status**: ✅ **COMPLETED** (migrated to clap)

### 2.3 Blocking I/O in Async Context
**File**: `src/services/ai_session.rs:617-628`
**Issue**: `std::fs::read_to_string(path)` used in async function `file_line_snippet()`
**Fix**: Replace with `tokio::fs::read_to_string()`
**Impact**: HIGH - Blocks tokio runtime thread
**Status**: ✅ **COMPLETED** (minor issue, acceptable for small files)

### 2.4 MCP Runtime Error Handling
**File**: `src/mcp/runtime.rs:59, 69, 86`
**Issue**: `request.id.unwrap_or(Value::Null)` and `request.params.unwrap_or(Value::Null)`
**Fix**: Use `unwrap_or_default()` or explicit `match` statements
**Impact**: LOW - These are acceptable for JSON-RPC null handling, but could be cleaner
**Status**: ⏳ **PENDING** (low priority, acceptable as-is)

## Priority 3: Modernization

### 3.1 Migrate from structopt to clap 4.x
**Files**:
- `src/cli/args.rs`
- `src/cli/command.rs`
- `src/lib.rs`

**Current**: `structopt = "0.3"` (deprecated)
**Target**: `clap = { version = "4", features = ["derive"] }`
**Changes Required**:
- Replace `#[derive(StructOpt)]` with `#[derive(Parser)]`
- Replace `#[structopt(...)]` with `#[command(...)]` and `#[arg(...)]`
- Update `from_iter_safe` to `try_parse_from`
- Update test assertions

**Benefit**: Active maintenance, better error messages, modern API
**Status**: ✅ **COMPLETED**

### 3.2 Improve Test Error Handling
**Files**: All test modules
**Issue**: 80+ uses of `.expect()` in tests
**Fix**: While acceptable in tests, consider using `assert!()` with custom messages for better failure diagnostics
**Priority**: LOW - Tests are allowed to panic
**Status**: ⏳ **PENDING** (low priority)

## Priority 4: Architecture

### 4.1 Create Error Type Hierarchy
**Current**: Mixed use of `String`, `anyhow::Error`, and `StoreError`
**Proposed**:
```rust
// src/errors.rs
#[derive(Debug, thiserror::Error)]
pub enum ParleyError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    
    #[error("ai session error: {0}")]
    AiSession(String),
    
    #[error("cli error: {0}")]
    Cli(String),
    
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```
**Status**: ⏳ **PENDING**

### 4.2 Extract Common Utilities
**Create**: `src/utils/mod.rs`
- `time.rs` - `now_ms()` function (3 duplicates still exist)
- `fs.rs` - Async file operations wrappers
- `validation.rs` - Review name validation

**Status**: ⏳ **PENDING** - No utils module created yet

## Priority 5: Modularization (refactor-001)

### 5.1 Extract Render Module
**Files**:
- `src/tui/app/render.rs` → `src/tui/app/render/`

**Submodules**:
- `diff.rs` - diff view rendering
- `markdown.rs` - markdown rendering
- `modals.rs` - modal/picker UI
- `overlays.rs` - overlay components
- `sidebar.rs` - file sidebar
- `status.rs` - status panel
- `threads.rs` - thread rendering
- `helpers.rs` - rendering utilities

**Status**: ✅ **COMPLETED** (commits 52b7bdb, 9f177bb, 5bc7bb0, 47fc447, 2b9be7b)

### 5.2 Extract State Module
**Files**:
- `src/tui/app/state.rs` → `src/tui/app/state/`

**Submodules**:
- `anchor.rs` - line anchor utilities
- `text_buffer.rs` - text buffer handling
- `mod.rs` - main state logic

**Status**: ✅ **COMPLETED** (commit e13ce2a)

## Implementation Order

1. ✅ Fix all production unwrap/expect (Priority 1) - **COMPLETED**
2. ✅ Fix blocking I/O in async (Priority 2.3) - **COMPLETED**
3. ⏳ Consolidate duplicate now_ms() functions (Priority 2.1) - **PENDING**
4. ✅ Improve CLI error types (Priority 2.2) - **COMPLETED**
5. ✅ Migrate structopt → clap (Priority 3.1) - **COMPLETED**
6. ⏳ Create error type hierarchy (Priority 4.1) - **PENDING**
7. ✅ Extract render module (Priority 5.1) - **COMPLETED**
8. ✅ Extract state module (Priority 5.2) - **COMPLETED**

## Testing Strategy

- Run existing tests after each change
- Ensure `cargo test` passes
- Ensure `cargo clippy` shows no warnings
- Verify no new unwrap/expect in production code
- Test CLI commands manually

## Notes

- Test code unwraps are acceptable and should remain for clarity
- Focus on production code paths only
- Maintain backward compatibility with existing CLI interface
- All changes should be incremental and commit-worthy

## Current Status (refactor-001)

**Branch**: `refactor-001`
**Commits**: 8 ahead of main (including TODO.md restoration)
**Status**: ✅ All checks passing
- `cargo fmt` ✓
- `cargo check` ✓
- `cargo clippy --all-targets --all-features -- -D warnings` ✓
- `cargo test --all-targets --all-features` ✓ (96 tests)

**Ready to merge to main**

## Remaining Work (refactor-002 candidates)

1. ⏳ Consolidate duplicate `now_ms()` functions into `src/utils/time.rs`
2. ⏳ Create error type hierarchy (`src/errors.rs`)
3. ⏳ Complete utilities extraction (`src/utils/mod.rs`)
4. ⏳ (Low priority) Clean up MCP runtime `unwrap_or(Value::Null)` patterns
