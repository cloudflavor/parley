# Parley Overview

Parley is a terminal-first review tool for local git diffs, optimized for iterative AI-assisted code review.

Review discussions are anchored to concrete diff lines, and both thread state and review state are explicit.

## Core model

- **Threaded line review**: each comment thread is anchored to file path + line reference.
- **Thread lifecycle**: `open`, `pending`, `addressed`.
- **Review lifecycle**: `open`, `under_review`, `done`.
- **Keyboard-first workflow**: full navigation and review operations without leaving the terminal.
- **Optional AI automation**: run AI thread replies/refactors while keeping state transitions human-controlled.

## Thread model at a glance

- A thread starts as `open` when a comment is created.
- Replies do not carry their own status; they update the parent thread status.
- When the original thread author replies, the thread becomes `open`.
- When a different author replies (including AI in normal flows), the thread becomes `pending`.
- `addressed` is explicit: the original thread author marks the thread resolved.

## Review model at a glance

- `open`: at least one thread is `open`.
- `under_review`: no `open` threads remain.
- `done`: explicitly set complete state.
- `done` is guarded: unresolved threads block normal transition to `done`.
- If unresolved threads appear after `done`, the review auto-reopens to `open`.

## AI eligibility summary

- AI runs are skipped when review state is `done`.
- `reply` mode targets `open` + `pending` threads by default.
- `refactor` mode targets `open` threads only.
- Explicitly selected thread IDs can override reply-mode filtering (details in `docs/review-workflow.md`).

## Typical session

```bash
parley review create my-review
parley review start my-review
parley tui --review my-review
```

If you are running over SSH and your terminal client does not play well with mouse reporting, start the TUI with `--no-mouse`.

## File references in comment drafts

Inside the inline comment or reply editor, `@` opens file matching against the current diff. Accepting a file switches the active diff pane to that file and enters a line-picker mode so you can move to or click the exact diff line before Parley inserts `@path:line` into the draft.

The editor itself calls out that a line still needs to be selected, and once the reference is inserted Parley restores the pane, file, and line where the draft originally started.

This keeps file references understandable to humans reading the thread instead of relying on a bare path plus a manually typed line number.

Inside that same draft editor, `Alt+b` moves backward by the previous whitespace-delimited word and `Alt+d` deletes forward through the next one.

## Local state and diff filtering

Parley stores reviews, logs, and config under `.parley/`.

Those `.parley/` files are ignored by default when Parley builds the review diff, so local review metadata does not pollute the file sidebar. This behavior is configurable through `.parley/config.toml`:

```toml
ignore_parley_dir = false
```

## What `pending` means

- `pending` indicates the thread is waiting on counterpart follow-up after a reply.
- A thread returns to `open` when the original author replies again or explicitly marks it open.

## Completion behavior

- `done` is blocked while unresolved threads remain.
- Use force done (`Shift+D` in TUI) only when intentionally closing with unresolved threads.
