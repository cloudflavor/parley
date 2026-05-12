# Refactor Roadmap

Purpose: reduce LOC, remove duplicated logic, improve performance, and increase focused tests without losing behavior. This file is written for an AI agent to consume one slice at a time.

Rules for every slice:

- Preserve behavior first. Delete or simplify only when tests prove the behavior still exists.
- Prefer borrowed inputs: `&str`, `&Path`, slices, and references.
- Avoid cloning domain objects just to inspect them. Clone only when ownership is required by storage, async tasks, or cache keys.
- Prefer iterator-returning helpers when the caller does not need ownership. Use `impl Iterator<Item = T>` or `impl Iterator<Item = &T>` where it keeps code readable.
- Avoid adding abstractions that do not remove duplicated code or reduce allocation.
- Add focused tests next to the changed behavior.
- Keep imports as leaf imports, one item per `use` line. Put `mod` declarations before imports.

## Current Hotspots

Clone-heavy files:

- `src/tui/app/state/anchor.rs`: 15 clones
- `src/tui/app/render/overlays.rs`: 15 clones
- `src/tui/app/state/viewport.rs`: 13 clones
- `src/tui/app/render/threads.rs`: 13 clones
- `src/tui/app/render/diff.rs`: 13 clones
- `src/tui/app/state/ai_session.rs`: 12 clones
- `src/git/diff.rs`: 9 clones
- `src/services/ai_session.rs`: 7 clones
- `src/services/ai_session/prompt.rs`: 7 clones

Allocation-heavy files:

- `src/tui/app/state/text_buffer.rs`: frequent `Vec<char>` collection for edits
- `src/tui/app/state/viewport.rs`: row/render cache cloning and temporary vectors
- `src/tui/app/state/file_navigation.rs`: visible index and group vectors cloned from cache
- `src/tui/app/render/diff.rs`: cloned cached render rows and cloned comments
- `src/git/diff.rs`: path and line vectors during root and diff parsing
- `src/domain/reference.rs`: parses through owned `Vec<char>` and owned strings
- `src/services/ai_session/prompt.rs`: hunk/range/snippet temporary vectors
- `src/git/history.rs`: duplicated path collection for heatmap

## Slice 1: Shared Search Backend

Files:

- `src/tui/app/input/search.rs`
- `src/tui/app/input/code_search.rs`
- New candidate: `src/tui/app/input/search_backend.rs`

Problem:

- File search and code search duplicate rg-first/grep-fallback command execution.
- Both files duplicate rg/grep output parsing into `CodeSearchResult`.
- Both have separate max-result truncation and parse tests.

Actions:

- Extract a shared backend with `SearchScope::Workspace` and `SearchScope::File(&str)`.
- Keep rg as primary engine and grep as fallback only when rg is missing.
- Share `parse_rg_output_line` and `parse_grep_output_line`.
- Return one `SearchRun { engine, results }` type used by both flows.
- Keep result collection bounded at 200.

Performance simplification:

- Avoid duplicate parser allocations.
- For grep workspace fallback, keep chunking but avoid building extra intermediate result vectors per parser where possible.

Tests:

- One parser test for rg output.
- One parser test for grep output.
- One test that file scope passes file paths through the shared parser.
- One test that workspace scope excludes `worktrees/`.

Acceptance:

- `search.rs` and `code_search.rs` no longer define their own rg/grep parsers.
- Existing `/` in-file search behavior remains current-file only.
- Global code search behavior remains available through the code search UI only.

## Slice 2: Borrowed Comment Accessors

Files:

- `src/tui/app/state/file_navigation.rs`
- `src/tui/app/state/thread_management.rs`
- `src/tui/app/render/diff.rs`

Problem:

- `comments_for_file` returns `Vec<&LineComment>`, forcing allocation in hot render paths.
- `comments_for_selected_file`, `selected_comment_details`, expanded/collapsed id lookups, and diff rendering repeatedly collect comments.
- `render/diff.rs` clones comments into `Vec<LineComment>` before rendering.

Actions:

- Add `comment_indices_for_file(&self, file_path: &str) -> impl Iterator<Item = usize> + '_`.
- Add `comments_for_file_iter(&self, file_path: &str) -> impl Iterator<Item = &LineComment> + '_`.
- Convert hot render paths to use iterators or small local index lists only when reuse requires it.
- Keep `comments_for_file` only if tests or low-volume UI paths still need a vector; otherwise remove it.

Performance simplification:

- Remove per-frame comment vector allocation in `draw_diff_view_for_pane`.
- Avoid cloning `LineComment` for render-only use.

Tests:

- Preserve existing tests for comment index rebuild.
- Add a test that selected comment lookup works after comment order changes.
- Add a render-path test around multiple comments in the same file.

Acceptance:

- No `.cloned().collect::<Vec<_>>()` for comments in `render/diff.rs`.
- `selected_comment_details` does not allocate a comment vector.

## Slice 3: Render Cache Borrowing

Files:

- `src/tui/app/render/diff.rs`
- `src/tui/app/state/viewport.rs`
- `src/tui/app/render/threads.rs`

Problem:

- Cached diff render entries are cloned out of the cache before viewport slicing.
- `last_diff_row_map` and link hit state are copied with `to_vec`.
- Highlight cache returns cloned highlight parts even when the caller only reads them.

Actions:

- Split render-cache usage into "borrow cached entry" and "build entry" paths.
- Render visible lines from borrowed cache slices where possible.
- Replace `row_map.to_vec()` with assignment only when ownership is needed after rendering.
- Evaluate whether `last_diff_row_map` can store the cache key plus viewport range instead of copying the full map every frame.
- Add a borrowed highlight accessor that returns `&[(Style, String)]` after ensuring cache population.

Performance simplification:

- Reduce full render-cache clones for every frame.
- Keep cache invalidation behavior unchanged.

Tests:

- Existing diff scroll tests must pass.
- Add a cache-hit test that verifies rendering uses cached rows without rebuilding thread bodies.

Acceptance:

- Cache hit path in `draw_diff_view_for_pane` does not clone `lines`, `row_map`, and `link_hits` just to compute scroll and visible lines.
- Thread expansion/collapse still invalidates the correct file cache.

## Slice 4: Anchor Projection Without Temporary Vectors

Files:

- `src/tui/app/state/anchor.rs`
- `src/services/ai_session/prompt.rs`

Problem:

- Anchor range projection collects row indices and normalized text into vectors before joining.
- Refreshing projections clones the entire comment list before iterating.
- `DiffSide` and `CommentLineRange` are cloned where copy/borrowed values would be enough.

Actions:

- Change `DiffSide` and `CommentStatus` to `Copy` if serde/domain usage allows it.
- Replace `refresh_comment_anchor_projections` clone of `review.comments` with index-based iteration or a two-phase collection of `(comment_id, projection)` only.
- Replace `exact_row_range_projection` temporary `Vec<usize>` with a single pass tracking last matching row and building projected text directly.
- Replace `selected_text_for_rows` and `file_content_text` vector-then-join with direct string builders.
- Share range matching helpers between TUI anchor code and AI prompt projection where practical.

Performance simplification:

- Remove cloning of all comments during projection refresh.
- Remove range projection temporary vectors.

Tests:

- Existing anchor projection tests must pass.
- Add a test for multi-line range projection preserving selected text comparison.

Acceptance:

- No `let comments = self.review.comments.clone()` in anchor projection refresh.
- No `collect::<Vec<_>>().join("\n")` in anchor text construction.

## Slice 5: Root/Diff Path Normalization Helpers

Files:

- `src/git/diff.rs`
- `src/git/history.rs`
- New candidate: `src/git/path.rs`

Problem:

- `normalize_relative_path` and `normalize_git_path` duplicate component-to-forward-slash logic.
- Both collect path components into vectors just to join.
- Root directory path collection creates temporary vectors from `BTreeSet`.

Actions:

- Extract a shared `normalize_repo_path(path: &Path) -> String`.
- Implement it with a direct string builder instead of `collect::<Vec<_>>().join("/")`.
- Reuse it in diff and history.
- In root source path collection, pass `Vec<PathBuf>` only where API ownership requires it; otherwise accept `impl IntoIterator<Item = PathBuf>`.

Performance simplification:

- Remove duplicate path normalization logic and component vectors.

Tests:

- Move existing normalization tests to the shared helper.
- Add absolute/non-normal component test if behavior exists today.

Acceptance:

- One path normalization implementation.
- `git/history.rs` no longer has its own normalization helper.

## Slice 6: TextBuffer Edit Efficiency

Files:

- `src/tui/app/state/text_buffer.rs`
- `src/tui/app/helpers.rs`

Problem:

- Single-character edits convert whole lines into `Vec<char>` on each keypress.
- Word movement converts the whole buffer to text and then to chars.
- This is a hot path for inline comments and prompts.

Actions:

- Add helper functions to convert character column to byte index for one line.
- Replace insert/delete/backspace/kill operations with `String::insert`, `replace_range`, `remove`, or `truncate` at byte boundaries.
- Keep Unicode correctness by using char-boundary byte indices.
- Keep whole-buffer conversion only for operations that genuinely span lines.

Performance simplification:

- Avoid per-keypress `Vec<char>` allocation for line-local edits.

Tests:

- Add tests for ASCII insert/delete/backspace.
- Add tests for multi-byte Unicode editing at char columns.
- Add tests for word-left/right across lines.

Acceptance:

- `insert_char`, `backspace`, `delete_char`, `kill_to_end`, and `replace_range_on_cursor_line` do not collect line chars into a vector.

## Slice 7: AI Session Targeting and Status Borrowing

Files:

- `src/services/ai_session.rs`
- `src/domain/review.rs`

Problem:

- Target selection clones `CommentStatus`.
- `comment_is_targetable` takes owned status.
- `comment_status` returns owned status.
- `AiSessionResult::new` clones strings from input/config even where formatting could be centralized.

Actions:

- Make simple enums `Copy` where appropriate: `Author`, `CommentStatus`, `DiffSide`, `ReviewState`.
- Change `comment_is_targetable(status: CommentStatus)` to use copied status or borrowed status consistently.
- Return `Option<CommentStatus>` by copy from `comment_status`.
- Keep `target_ids` owned because async processing mutates review state between targets.
- Review `AiSessionResult::new` for only necessary owned output fields.

Performance simplification:

- Remove status clones and reduce enum ownership friction across services/TUI.

Tests:

- Existing AI session targetability tests must pass.
- Add a test that targeted addressed comments are skipped and open/pending are processed.

Acceptance:

- No `.clone()` on `CommentStatus`, `Author`, or `DiffSide` just to match or pass by value.

## Slice 8: Prompt Context Builders

Files:

- `src/services/ai_session/prompt.rs`
- `src/domain/reference.rs`

Problem:

- Snippet helpers collect all file lines into `Vec<&str>` before slicing.
- Hunk choice collects scored hunks into a vector and sorts only to find minimum distance.
- Referenced file parsing returns owned refs even when callers only need path/line during iteration.

Actions:

- Replace `choose_best_hunk` scoring vector with `min_by_key`.
- Replace file snippet `Vec<&str>` collection with streaming line enumeration and bounded context capture.
- Add `parse_file_references_iter(input: &str) -> impl Iterator<Item = FileReference>` only if it does not make parsing harder to read.
- If iterator parsing is too complex, keep owned parser but remove duplicate downstream collections.

Performance simplification:

- Avoid reading full file lines into a second vector for small snippets.
- Avoid sorting when only minimum distance is needed.

Tests:

- Existing prompt excerpt tests must pass.
- Add tests for line snippets near file start/end and range snippets.

Acceptance:

- `choose_best_hunk` does not allocate.
- Snippet helpers do not collect all lines before selecting context.

## Slice 9: MCP Tool Dispatch Table

Files:

- `src/mcp/runtime.rs`

Problem:

- Tool schemas are embedded in a long `json!` block.
- Tool call dispatch mixes argument parsing, service calls, and response wrapping in one function.
- Documentation resource metadata is collected into vectors in multiple places.

Actions:

- Extract tool schema construction into small named functions.
- Extract one function per tool handler.
- Add `documentation_resources() -> impl Iterator<Item = Value>` or a small helper used by both resource list and docs tool.
- Keep JSON response shape unchanged.

Performance simplification:

- This is mostly LOC and maintainability. Avoid over-abstracting into trait objects.

Tests:

- Existing MCP tests must pass.
- Add one test per extracted tool handler only where behavior is not already covered.

Acceptance:

- `handle_tools_call` is a short dispatcher.
- Tool schema JSON is not embedded as one large block.

## Slice 10: Provider Client Cache Keys

Files:

- `src/services/ai_session/provider/acp.rs`
- `src/services/ai_session/provider/pi_rpc.rs`
- `src/domain/config.rs`

Problem:

- ACP and Pi RPC build client cache keys with repeated `format!` and joined args.
- Both client caches use similar `OnceCell<Mutex<HashMap<String, Arc<Mutex<_>>>>>` patterns.
- Provider command profile conversion clones args into `Vec<String>`.

Actions:

- Extract a small cache-key helper that takes cwd, client, and `&[String]`.
- Consider a generic `get_or_spawn_client` only if it removes net code without obscuring provider differences.
- Change command profile helpers to expose static slices and clone only at final config construction.

Performance simplification:

- Reduce duplicated cache-key code and unnecessary string assembly helpers.

Tests:

- Existing provider tests must pass.
- Add test for cache key stability with same args and different cwd/client.

Acceptance:

- Cache key construction exists once.
- Provider-specific protocol handling stays provider-specific.

## Slice 11: Store Legacy Compatibility Boundary

Files:

- `src/persistence/store.rs`

Problem:

- Flat review compatibility remains mixed with normal list/load flow.
- Config JSON compatibility is already removed; flat review compatibility should be isolated or removed by policy.

Actions:

- Decide whether flat `.parley/reviews/<name>.json` compatibility still matters.
- If keeping it, move all legacy review helpers into a clearly named section or module.
- If removing it, delete `load_legacy_review`, `legacy_review_name`, `legacy_review_path`, and the legacy test.

Performance simplification:

- Normal review listing should only walk per-review directories if legacy is removed.

Tests:

- If kept, legacy load/list test remains.
- If removed, add a test that flat files are ignored.

Acceptance:

- Normal storage path is visually direct.
- Legacy behavior is either gone or isolated.

## Slice 12: TUI File Navigation Caches

Files:

- `src/tui/app/state/file_navigation.rs`
- `src/tui/app/render/sidebar.rs`

Problem:

- `visible_file_indices` returns a cloned cached vector.
- `ordered_visible_file_groups` rebuilds grouped vectors on each call.
- Selection movement should use all visible files, but grouping/rendering also needs viewport-limited row maps.

Actions:

- Add borrowed accessor for cached visible indices: `visible_file_indices_ref`.
- Keep a separate owned computation only on cache miss.
- Consider caching ordered groups with a key that includes filter/sort/query/collapsed groups if render profiling shows repeated rebuilds.
- Do not reintroduce viewport-limited navigation bugs.

Performance simplification:

- Avoid clone-on-read for visible file indices.
- Keep navigation based on full visible data, not rendered viewport rows.

Tests:

- Existing large-project navigator test must pass.
- Add a test that cache hit returns stable visible order after unrelated scroll changes.

Acceptance:

- Moving file selection does not allocate through a full cloned visible list on every keypress unless needed for mutation safety.

## Suggested Order

1. Slice 7: copy small enums and remove low-risk clones.
2. Slice 5: shared git path helper.
3. Slice 8: no-allocation hunk/snippet helpers.
4. Slice 1: shared search backend.
5. Slice 2: borrowed comment accessors.
6. Slice 4: anchor projection no temporary vectors.
7. Slice 12: file navigation cache borrowing.
8. Slice 3: render cache borrowing.
9. Slice 6: TextBuffer edit efficiency.
10. Slice 9: MCP dispatch cleanup.
11. Slice 10: provider cache-key helper.
12. Slice 11: storage compatibility decision.

## Measurement Checklist

Run before and after each slice:

- `cargo fmt`
- `cargo check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`
- `rg -n "\\.clone\\(\\)" src`
- `rg -n "collect::<Vec|\\.collect\\(\\)" src`

Do not chase zero clones or zero collects. Some ownership is correct for persisted models, async task boundaries, cache keys, and UI-owned render lines.
