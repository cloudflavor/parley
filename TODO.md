# Remaining TODO

## Cleanup audit: duplicated logic and agent transports

Primary goal: remove lines and simplify control flow without losing functionality. Prefer deleting duplicated branches, centralizing shared behavior, and shrinking provider-specific code over adding new abstractions. Add an abstraction only when it removes more code than it creates or prevents the same logic from staying duplicated.

Implementation rule: each cleanup slice should preserve current behavior first, then remove dead/duplicated code in the same slice. Keep tests focused on behavior that must not regress.

### Completed Review Follow-Up

- [x] Address review `new`: merge `src/services/ai_session.rs` local module imports into the main import group.
- [x] Address review `new`: split `run_ai_session_inner` into result construction, target selection, target iteration, and single-target processing helpers.

### Start Here

Start with units 1-4. They are mostly deletion/renaming work, have small blast radius, and make later provider cleanup easier to review.

Do not start with shared provider streaming until the low-risk dead branches and naming cleanup are done.

### Unit 1: Remove Misleading "Legacy CLI" Naming

- [x] Rename `is_legacy_non_acp_command` to describe the actual invalid state: ACP transport configured with a CLI command shape.
- [x] Rename tests that call CLI provider commands "legacy".
- [x] Keep behavior unchanged.

Files:
- `src/domain/config.rs`
- `src/services/ai_session/provider.rs`

Relevant code:
- `src/domain/config.rs:201`
- `src/domain/config.rs:237`
- `src/domain/config.rs:386`
- `src/services/ai_session/provider.rs:547`

Success criteria:
- No "legacy" terminology remains for normal CLI invocation.
- Existing ACP validation and config repair tests still pass.
- Net code should be same or smaller except names.

### Unit 2: Delete Ignored Prompt Transport Configuration

- [x] Confirm all supported providers use argv prompt passing.
- [x] Remove `PromptTransport::Stdin`, `PromptTransport`, and `AiProviderConfig::prompt_transport` if stdin is not supported.
- [x] Delete `normalized_prompt_transport`.
- [x] Delete stdin prompt-writing branches in CLI provider invocation.
- [x] Update config tests and defaults.

Files:
- `src/domain/config.rs`
- `src/services/ai_session/provider.rs`

Relevant code:
- `src/domain/config.rs:38`
- `src/domain/config.rs:74`
- `src/services/ai_session/provider.rs:106`
- `src/services/ai_session/provider.rs:141`
- `src/services/ai_session/provider.rs:518`

Success criteria:
- No config field exists that is ignored for every provider.
- Provider invocation has one prompt-passing path.
- The diff removes more code than it adds.

### Unit 3: Collapse AI Targetability Logic

- [x] Remove the `AiSessionMode` parameter from `comment_is_targetable` if `Reply` and `Refactor` remain identical.
- [x] Replace duplicated tests with one targetability test covering open, pending, and addressed.
- [x] Keep user-facing skipped messages mode-specific only where text differs.

Files:
- `src/services/ai_session.rs`
- `src/services/ai_session/tests.rs`

Relevant code:
- `src/services/ai_session.rs:139`
- `src/services/ai_session.rs:212`
- `src/services/ai_session.rs:548`

Success criteria:
- No duplicated `Reply`/`Refactor` match arms with identical logic.
- Target selection behavior is unchanged.
- Tests are shorter and still cover addressed-thread exclusion.

### Unit 4: Remove Stale CLI/Docs Entries

- [x] Remove `resolve <name>` from README unless a command is intentionally added.
- [x] Decide whether `review start` is kept as a shortcut or removed as old lifecycle surface.
- [x] If keeping `review start`, document it as a shortcut to `set-state <name> under_review`.
- [x] Kept `review start`, so no clap variant, handler branch, docs, or tests were removed.

Files:
- `README.md`
- `src/cli/command.rs`
- `src/lib.rs`

Relevant code:
- `README.md:126`
- `src/cli/command.rs:51`
- `src/lib.rs:106`

Success criteria:
- CLI docs match actual clap commands.
- No stale command is documented.
- If a command is removed, the deletion is explicit and covered by parser tests.

### Unit 5: Centralize Domain String Parsing And Formatting

- [x] Add `as_str` and/or `FromStr` implementations for `ReviewState`, `Author`, `DiffSide`, and `CommentStatus` where useful.
- [x] Replace CLI wrapper parsing literals with domain parsing.
- [x] Replace MCP parsing literals with domain parsing.
- [x] Replace inline CLI status display mapping with domain formatting.

Files:
- `src/domain/review.rs`
- `src/cli/args.rs`
- `src/mcp/runtime.rs`
- `src/lib.rs`

Relevant code:
- `src/cli/args.rs:23`
- `src/cli/args.rs:50`
- `src/mcp/runtime.rs:493`
- `src/mcp/runtime.rs:502`
- `src/lib.rs:127`

Success criteria:
- State/author/status/side string literals live in domain types.
- CLI and MCP parsing paths share behavior.
- Net line count should decrease or stay close while removing duplicate literals.

### Unit 6: Simplify Review Mutation Boilerplate

- [ ] Add a private `mutate_review` helper in `ReviewService`.
- [ ] Use it for `set_state`, `add_reply`, `force_mark_addressed`, `reanchor_comment`, and `set_comment_status`.
- [ ] Keep method-specific context messages if they add useful failure location.

Files:
- `src/services/review_service.rs`

Relevant code:
- `src/services/review_service.rs:131`
- `src/services/review_service.rs:173`
- `src/services/review_service.rs:217`
- `src/services/review_service.rs:233`
- `src/services/review_service.rs:270`

Success criteria:
- Each public mutation method contains only input mapping plus its unique domain operation.
- Load/mutate/save boilerplate exists once.
- Error messages remain actionable.

### Unit 7: Isolate Or Remove Old Storage Compatibility

- [ ] Decide whether flat `.parley/reviews/<name>.json` and `.parley/config.json` reads are still required.
- [ ] If not required, delete legacy path helpers and tests.
- [ ] If still required, isolate compatibility reads in a small migration/helper section so normal load/list paths stay direct.

Files:
- `src/persistence/store.rs`

Relevant code:
- `src/persistence/store.rs:80`
- `src/persistence/store.rs:111`
- `src/persistence/store.rs:142`
- `src/persistence/store.rs:176`
- `src/persistence/store.rs:203`

Success criteria:
- Normal persistence path is easy to read: directory review JSON plus `config.toml`.
- Legacy compatibility is either gone or visibly isolated.
- Line count decreases if compatibility is removed.

### Unit 8: Centralize Provider Command Profiles

- [ ] Introduce one profile source for `(provider, transport)` command shape.
- [ ] Use it for default config, transport override config, and ACP command validation/replacement.
- [ ] Keep CLI invocation generic: provider differences should be profile data and reply extraction only.
- [ ] Keep ACP command differences as profile data.

Files:
- `src/domain/config.rs`
- `src/services/ai_session/provider.rs`

Relevant code:
- `src/domain/config.rs:151`
- `src/domain/config.rs:255`
- `src/domain/config.rs:290`
- `src/services/ai_session/provider.rs:296`

Success criteria:
- Fewer provider command branches and repeated command literals.
- CLI command setup is generic.
- ACP command validation uses the same command profile data as defaults.

### Unit 9: Decide Pi Transport Shape

- [ ] Decide whether `PiRpc` belongs in generic `AgentTransport`.
- [ ] If user-facing agent invocation is only CLI or ACP, move Pi RPC out of the generic transport toggle/config path.
- [ ] If Pi RPC remains, document it as provider-specific and non-agent-generic.

Files:
- `src/domain/config.rs`
- `src/tui/app/state/ai_session.rs`
- `src/services/ai_session/provider/pi_rpc.rs`

Relevant code:
- `src/domain/config.rs:38`
- `src/domain/config.rs:166`
- `src/tui/app/state/ai_session.rs:736`

Success criteria:
- Generic agent transport model exposes only real generic choices.
- Pi special-casing is reduced or explicitly isolated.

### Unit 10: Replace Pi RPC Line Loops With Tokio Streams

- [ ] Replace Pi RPC `next_line()` loops with `tokio_stream::wrappers::LinesStream` and `StreamExt`.
- [ ] Preserve logging behavior and stdout JSON parse behavior.
- [ ] Surface read errors consistently instead of silently dropping them.

Files:
- `src/services/ai_session/provider/pi_rpc.rs`

Relevant code:
- `src/services/ai_session/provider/pi_rpc.rs:125`
- `src/services/ai_session/provider/pi_rpc.rs:135`

Success criteria:
- No `next_line()` process stream loops remain in Pi RPC.
- Behavior remains equivalent or reports errors better.
- This unit prepares, but does not require, the shared streaming helper.

### Unit 11: Share AI Process Streaming

- [ ] Extract a shared line-streaming/logging helper around `LinesStream` and `StreamExt`.
- [ ] Use it for ACP stderr logging.
- [ ] Use it for Pi RPC stderr logging.
- [ ] Use it for ACP/Pi JSON stdout parsing with provider/prefix differences as parameters.
- [ ] Use the same primitive for one-shot CLI provider stdout/stderr collection.
- [ ] Leave MCP `read_line()` framing alone.

Files:
- `src/services/ai_session/provider.rs`
- `src/services/ai_session/provider/acp.rs`
- `src/services/ai_session/provider/pi_rpc.rs`

Relevant code:
- `src/services/ai_session/provider.rs:151`
- `src/services/ai_session/provider.rs:488`
- `src/services/ai_session/provider/acp.rs:146`
- `src/services/ai_session/provider/acp.rs:170`
- `src/services/ai_session/provider/pi_rpc.rs:122`
- `src/services/ai_session/provider/pi_rpc.rs:132`
- `src/mcp/runtime.rs:130`
- `src/mcp/runtime.rs:175`

Success criteria:
- One process line-streaming implementation supports CLI collection and agent process logging.
- ACP and Pi RPC no longer contain separate stdout/stderr loop implementations.
- MCP protocol framing remains unchanged.

### Unit 12: Share ACP Protocol Structures

- [ ] Introduce ACP protocol request/response/event structs if they remove ad hoc JSON handling.
- [ ] Keep provider differences limited to command/profile setup and genuinely provider-specific event interpretation.
- [ ] Avoid one ACP JSON parser per agent.

Files:
- `src/services/ai_session/provider/acp.rs`

Relevant code:
- `src/services/ai_session/provider/acp.rs`

Success criteria:
- ACP protocol handling is represented once.
- Provider setup is outside protocol parsing.
- Less ad hoc JSON assembly/parsing where typed structs are clearer.

### Unit 13: Share JSON Text Extraction And Redaction

- [ ] Create a shared JSON text extraction utility for provider and TUI AI logs.
- [ ] Move ACP `extract_text` into it.
- [ ] Move Pi RPC text/thought/named-text walking into it where behavior matches.
- [ ] Move TUI AI log text-fragment extraction into it where behavior matches.
- [ ] Make JSON log redaction reusable.

Files:
- `src/services/ai_session/provider/acp.rs`
- `src/services/ai_session/provider/pi_rpc.rs`
- `src/tui/app/state/ai_session.rs`
- `src/services/ai_session/json_text.rs` if a new module still removes net duplication

Relevant code:
- `src/services/ai_session/provider/acp.rs:695`
- `src/services/ai_session/provider/acp.rs:724`
- `src/services/ai_session/provider/pi_rpc.rs:318`
- `src/services/ai_session/provider/pi_rpc.rs:502`
- `src/services/ai_session/provider/pi_rpc.rs:543`
- `src/tui/app/state/ai_session.rs:801`

Success criteria:
- Recursive JSON walking code exists in one place, not three.
- Redaction behavior is consistent across provider logs.
- New module removes more duplicated code than it adds.

### Unit 14: Deduplicate Single-Line TUI Editing

- [ ] Reuse one single-line text input helper for file search and command prompts.
- [ ] Preserve cursor movement, home/end, backspace/delete, and character insertion behavior.
- [ ] Use existing helper functions or a small focused abstraction; avoid expanding `TextBuffer` unless it removes net code.

Files:
- `src/tui/app/input/search.rs`
- `src/tui/app/helpers.rs`

Relevant code:
- `src/tui/app/input/search.rs:13`
- `src/tui/app/input/search.rs:68`
- `src/tui/app/helpers.rs:314`

Success criteria:
- Single-line prompt/search editing behavior is implemented once.
- Tests cover at least one file-search and one command-prompt edit path.

### Unit 15: Deduplicate Terminal Leave/Restore

- [ ] Factor terminal leave/restore around pager and suspend actions.
- [ ] Use one helper that leaves raw/alternate-screen mode, runs an action, restores terminal state, and clears.
- [ ] Preserve mouse capture behavior.

Files:
- `src/tui/app/helpers.rs`

Relevant code:
- `src/tui/app/helpers.rs:190`
- `src/tui/app/helpers.rs:236`

Success criteria:
- Terminal leave/restore sequence exists in one place.
- Pager and suspend behavior remain unchanged.

### Required verification after each code change

```bash
cargo fmt && cargo check && cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

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
