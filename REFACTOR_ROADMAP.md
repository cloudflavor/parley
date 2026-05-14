# Refactor Roadmap

**Last Updated:** 2026-05-14  
**Total Clone Calls:** 211 across codebase  
**Primary Focus:** Reduce unnecessary allocations, prefer borrowed accessors, improve render path efficiency

Purpose: reduce LOC, remove duplicated logic, improve performance, and increase focused tests without losing behavior. This file is written for an AI agent to consume one slice at a time.

## Best Practices Reference (from rust-best-practices skill)

### Borrowing & Ownership
- Prefer `&T` over `.clone()` unless ownership transfer is required
- Use `&str` over `String`, `&[T]` over `Vec<T>` in function parameters
- Small `Copy` types (≤24 bytes) can be passed by value
- Use `Cow<'_, T>` when ownership is ambiguous

### Performance
- Always benchmark with `--release` flag
- Run `cargo clippy --all-targets --all-features --locked -- -D warnings`
- Avoid cloning in loops; use `.iter()` instead of `.into_iter()` for Copy types
- Prefer iterators over manual loops; avoid intermediate `.collect()` calls

### Key Lints
- `redundant_clone` - unnecessary cloning
- `large_enum_variant` - oversized variants (consider boxing)
- `needless_collect` - premature collection

---

## Current Hotspots (Updated Metrics)

### Clone-Heavy Files (211 total clone calls)

| File | Clone Count | Primary Patterns |
|------|-------------|------------------|
| `src/tui/app/state/anchor.rs` | 15 | String clones for file_path, selected_text; Side enum clones; review.comments full vector clone |
| `src/tui/app/render/overlays.rs` | 15 | theme().colors.clone() (9×); query/result snapshot clones |
| `src/tui/app/state/viewport.rs` | 13 | DisplayRow field clones in row cache build; cache key clones |
| `src/tui/app/state/ai_session.rs` | 13 | String clones for file_path, review_name; session vector clones |
| `src/tui/app/render/threads.rs` | 13 | theme().colors.clone(); indent_str.clone() (5×); comment clones in tests |
| `src/tui/app/render/diff.rs` | 13 | theme().colors.clone(); cached render entry clones; file path clones |
| `src/tui/app/render/modals.rs` | 11 | (needs analysis) |
| `src/tui/app/state/review.rs` | 10 | (needs analysis) |
| `src/git/diff.rs` | 10 | config.clone() (4×) for worker threads; tree/hunk header clones |
| `src/tui/app/state/settings.rs` | 9 | (needs analysis) |

### Allocation-Heavy Files (collect::<Vec> patterns)

| File | Vec Collections | Primary Patterns |
|------|-----------------|------------------|
| `src/git/diff.rs` | 5 | content.lines().collect(); path filtering; test parent refs |
| `src/tui/app/state/viewport.rs` | 4 | row cache building; highlight vectors |
| `src/tui/app/state/thread_management.rs` | 4 | comment filtering; status updates |
| `src/tui/app/state/anchor.rs` | 4 | selected_text_for_rows; file_content_text; hunk_lines collection |
| `src/tui/app/render/diff.rs` | 4 | comment collection; search results; line character splitting |
| `src/git/history.rs` | 4 | heatmap entries; path filtering; commit parent refs |
| `src/tui/app/state/file_navigation.rs` | 3 | visible indices; group vectors |
| `src/tui/app/render/threads.rs` | 3 | thread body lines; comment rendering |
| `src/services/ai_session/prompt.rs` | 3 | anchor projection lines; hunk scoring |

---

## Detailed Per-File Analysis

### anchor.rs (15 clones, 4 collects)

**Clone locations:**
- Line 27: `file.path.clone()` - String for StoredAnchorSnapshot
- Line 32: `selected_text.clone()` - String already computed locally
- Line 51: `self.review.comments.clone()` - **HOTSPOT**: full comment vector clone for iteration
- Lines 127, 228, 231, 237, 240, 255, 293: `DiffSide` enum clones (likely Copy candidate)
- Line 232: `anchor.selected_text.clone()` - conditional String clone
- Lines 390-391: `hunk.header.clone()` and hunk line collection
- Lines 408-409: revision String clones for DiffSource variants

**Vec collection locations:**
- Lines 270, 276: `selected_text_for_rows` and context window building
- Lines 375, 383: `file_content_text` and row iteration
- Line 391: `hunk.lines.iter().map(...).collect()` for snapshot

**Recommendations:**
1. Change `refresh_comment_anchor_projections` to iterate over `&self.review.comments` instead of cloning
2. Check if `DiffSide` can derive `Copy` (likely ≤24 bytes)
3. Return `impl Iterator` from helper functions where ownership not required
4. Use `Cow<'_, str>` for `selected_text` fields that may be borrowed

---

### overlays.rs (15 clones, mostly theme colors)

**Clone locations:**
- Lines 21, 87, 217, 319, 461, 725, 816, 880, 989: `app.theme().colors.clone()` (9×) - **CRITICAL HOTSPOT**
- Line 115: `selector.query.clone()` - String for thread selector
- Line 480: `heatmap.entries.clone()` - vector clone for overlay
- Lines 867, 973, 977, 978: snapshot query/result clones

**Recommendations:**
1. **Highest priority**: Change `theme().colors` to return `&ThemeColors` reference
2. All render functions take `&ThemeColors` parameter instead of cloning
3. Snapshot queries can use `&str` references where lifetime permits
4. `heatmap.entries` iteration should use borrowed accessor

---

### viewport.rs (13 clones, 4 collects)

**Clone locations:**
- Lines 244, 248: URL parts parsing and slot assignment
- Lines 310-311, 328, 331-332: `DisplayRow` field clones during row cache build - **HOTSPOT**
- Lines 377, 403: cache key clones for diff/thread render cache
- Line 689: `row.code.clone()` for render output
- Lines 720-721, 734: test cache key clones

**Vec collection locations:**
- Line 427: row cache highlights initialization
- Lines 615, 627, 659: various render helper collections

**Recommendations:**
1. `DisplayRow` build in `rebuild_row_cache_for_file` should borrow from source `DiffFile`
2. Cache keys can use `Rc<String>` or `Arc<str>` if shared ownership needed
3. Consider `Cow<'_, str>` for `DisplayRow.code` field

---

### threads.rs (13 clones, 3 collects)

**Clone locations:**
- Lines 43, 675: `app.theme().colors.clone()` (render functions)
- Line 47: `comment.side.clone()` for anchor formatting
- Lines 260, 278, 288, 296, 315: `indent_str.clone()` (5×) - String clone in loop
- Line 326: `span.content.clone()` for styled spans
- Lines 436, 444: thread body cache get/set clones
- Lines 638, 727: test comment clones

**Recommendations:**
1. `indent_str` clones in render loops should be `&str` or pre-computed once
2. Thread body cache should store `Arc<[Line]>` or borrowed content
3. `comment.side` likely Copy candidate

---

### diff.rs (render) (13 clones, 4 collects)

**Clone locations:**
- Line 41: `app.theme().colors.clone()` - render function
- Line 54: `file.path.clone()` for file path extraction
- Line 118: `search_query.clone()` for render state
- Lines 130-132: cached render entry clones (lines, row_map, link_hits)
- Lines 351-353: cache entry field clones
- Lines 551, 557: `row.raw.clone()` for span building
- Lines 1016, 1049: help line and content line clones

**Vec collection locations:**
- Line 143: comment collection for rendering
- Line 220: search result collection
- Line 399: highlight span collection
- Line 1111: `line.chars().collect()` for character indexing

**Recommendations:**
1. Cache entries should store `Arc<Lines>` or borrowed references
2. `row.raw` can be `&str` in render path
3. Search highlights should use borrowed accessors

---

### ai_session.rs (state) (13 clones)

**Clone locations:**
- Line 152: `session.file_path.clone()` for UI state
- Line 338: session file path mapping
- Line 345: `session.clone()` for pending queue
- Lines 358, 370: file path clones for navigation
- Lines 464, 467, 549: task file path clones for event handling
- Lines 674, 679-680, 683, 692: session init and service clones

**Recommendations:**
1. File paths can use `Arc<str>` for shared ownership
2. Session queue can use references with proper lifetimes
3. Service clone may be necessary for async boundaries (verify)

---

### git/diff.rs (10 clones, 5 collects)

**Clone locations:**
- Lines 48-49: `source.clone()`, `config.clone()` for worker thread - **necessary for async**
- Line 108: `tree.clone()` for git tree access
- Lines 155-156: `config.clone()`, `relative_path.clone()` for blocking task
- Lines 256, 279: `config.clone()` for path filtering workers
- Line 405: `display_path.clone()` for DiffFile construction
- Lines 441-442: `hunk.header.clone()` for placeholder hunks

**Vec collection locations:**
- Line 427: `content.lines().collect()` for file parsing
- Line 488: path component filtering
- Lines 1057, 1146-1147: test path/parent collections

**Recommendations:**
1. Worker thread clones are **necessary** for `spawn_blocking` - document this
2. `hunk.header` clones for placeholders may be avoidable with borrowed construction
3. Path filtering can use iterator chaining without intermediate collect

---

### services/ai_session/prompt.rs (7 clones, 3 collects)

**Clone locations:**
- Line 354: `path.clone()` for file marker formatting
- Lines 395, 401-402, 438-439, 442: anchor field clones for projection
- Line 442: `range.clone()` for line range

**Vec collection locations:**
- Lines 422, 429: anchor projection line filtering and text collection
- Line 505: hunk scoring vector for sorting

**Recommendations:**
1. `CurrentAnchorProjection` can use `&str` for `file_path` with lifetime
2. Line filtering can return `impl Iterator` instead of collecting
3. Hunk scoring can use slice sorting without intermediate vector

---

## Slice Priority (Updated)

Based on clone concentration and risk assessment:

1. **Slice 0: Theme Colors Borrowing** (NEW - highest impact, lowest risk)
   - Files: `src/tui/app/render/overlays.rs`, `src/tui/app/render/diff.rs`, `src/tui/app/render/threads.rs`
   - Impact: Removes 9+ unnecessary clones from render paths
   - Actions: Change `theme().colors` to return `&ThemeColors`, update all render functions

2. **Slice 7: Copy Enums and Small Types**
   - Files: `src/domain/diff.rs`, `src/domain/review.rs`
   - Check `DiffSide`, `DiffLineKind`, `CommentStatus` for Copy derivation

3. **Slice 5: Shared Git Path Helper**

4. **Slice 8: No-Allocation Hunk/Snippet Helpers**

5. **Slice 2: Borrowed Comment Accessors** (upgraded priority)
   - Files: `src/tui/app/state/file_navigation.rs`, `src/tui/app/state/thread_management.rs`, `src/tui/app/render/diff.rs`
   - Impact: Removes `comments.clone()` in anchor.rs line 51 and render paths

6. **Slice 1: Shared Search Backend**

7. **Slice 4: Anchor Projection No Temporary Vectors**

8. **Slice 12: File Navigation Cache Borrowing**

9. **Slice 3: Render Cache Borrowing**

10. **Slice 6: TextBuffer Edit Efficiency**

11. **Slice 9: MCP Dispatch Cleanup**

12. **Slice 10: Provider Cache-Key Helper**

13. **Slice 11: Storage Compatibility Decision**

---

## Measurement Checklist

Run before and after each slice:

```bash
cargo fmt
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
rg -n "\.clone\(\)" src --count | sort -t: -k2 -rn
rg -n "collect::<Vec" src --count | sort -t: -k2 -rn
```

Do not chase zero clones or zero collects. Some ownership is correct for:
- Persisted models
- Async task boundaries
- Cache keys
- UI-owned render lines
- Worker thread inputs (required for `spawn_blocking`)
