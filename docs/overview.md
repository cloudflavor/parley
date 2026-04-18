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

## What `pending` means

- `pending` indicates the thread is waiting on counterpart follow-up after a reply.
- A thread returns to `open` when the original author replies again or explicitly marks it open.

## Completion behavior

- `done` is blocked while unresolved threads remain.
- Use force done (`Shift+D` in TUI) only when intentionally closing with unresolved threads.
