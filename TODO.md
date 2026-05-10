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
