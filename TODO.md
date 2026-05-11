# Remaining TODO

## Performance audit follow-up

Benchmark command:

```bash
cargo test --release perf_ -- --ignored --nocapture
```

Current baseline:

- `tui_draw_large_review`: 0.860ms per draw
- `visible_file_indices_many_files_and_comments`: 0.334ms per run
- `rebuild_row_cache_large_file`: 536.125ms per rebuild

Priority work:

1. Lazy-highlight visible rows instead of full-file syntax highlighting during row cache rebuild.
2. Cache comments by file path so diff/thread rendering does not scan every review comment.
3. Cache file stats and visible file indices until comments/filter/sort/search state changes.
4. Avoid cloning full diff render cache entries on cache hits.
5. Make root mode load file lists first, then load file content lazily or with bounded concurrency.
6. Index comment anchors by file/line to reduce refresh/remap scans.
7. Cache wrapped/rendered thread bodies by width plus thread revision.

Keep the ignored perf tests updated with each optimization so regressions are measurable.

## Thread anchor follow-up

Goal: make thread anchoring behave like GitHub outdated diff comments without silently moving comments to the wrong code.

Design rules:

1. Treat `thread_id` as identity. Never use selected row, file order, or file-local comment index as identity.
2. Keep the original anchor immutable after comment creation.
3. Store a creation-time anchor snapshot:
   - file path
   - side
   - old/new line or selected line range
   - selected text
   - before/after context
   - diff hunk header and hunk lines in diff mode
   - file content hash and selected text hash in root mode
   - base/head identity when available
4. Compute current projection separately from the stored original anchor.
5. Only render inline as exact when the current file/diff still matches the stored anchor exactly.
6. If exact mapping fails, render the thread as detached/outdated with the stored original context.
7. Do not automatically fuzzy-reanchor comments. Fuzzy matching may suggest a possible current location, but user action must accept a permanent reanchor.
8. AI prompts must include thread id, anchor status, original context, and optional current projection with confidence. Agents must reply to the exact thread id only.

Root mode specifics:

1. Use source snapshots instead of diff hunks.
2. If the original line range no longer matches the stored selected text, mark the anchor outdated.
3. If the same selected text exists elsewhere, show it as a possible projection, not as the canonical anchor.
4. If the file is missing or renamed, keep the thread visible in the thread selector and detached/outdated section.

Implementation slices:

1. Add stored anchor snapshot fields and migration/default handling for existing reviews.
2. Capture snapshots when creating comments in root mode and diff mode.
3. Add exact projection computation that never mutates the original anchor.
4. Render detached/outdated thread context when projection is missing.
5. Update AI prompt context to include anchor status and original/current projection data.
6. Add tests for refactored lines, deleted lines, moved text, same-file multiple threads, and root-mode file changes.

## Refactoring follow-up

Source: merged from the `t3code/b1b92b25` worktree refactoring notes.

Completed items from that worktree:

1. Removed production `unwrap`/`expect` paths in AI session, TUI input, TUI helpers, and related state handling.
2. Consolidated duplicate time helpers into `src/utils/time.rs`.
3. Kept small prompt template reads as acceptable for now; revisit only if profiling shows measurable impact.

Pending items:

1. Migrate CLI parsing from `structopt` to `clap` 4 derive APIs.
2. Replace stringly CLI parse errors with a typed error enum.
3. Review MCP runtime JSON-RPC optional field handling for clearer default/null behavior.
4. Consider a project error hierarchy if `anyhow` contexts stop being sufficient at service boundaries.
5. Extract reusable async filesystem and validation helpers only where repeated call sites justify it.
