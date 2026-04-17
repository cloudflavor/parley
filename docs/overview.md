# Parley Overview

Parley allows you to review your AI agents changes fast.

Parley is a terminal-first code review tool for local git changes.

It keeps review discussion structured around threads on diff lines, with explicit thread states and review-level state transitions.

## Core ideas

- **Threaded review**: comments are anchored to file + line context.
- **Stateful workflow**: each thread and review has a lifecycle.
- **Keyboard-driven TUI**: fast navigation and thread handling without leaving the terminal.
- **Optional AI runs**: generate threaded replies/refactors while keeping human state control.

## Session lifecycle

```bash
parley review create my-review
parley review start my-review
parley tui --review my-review
```

## Thread states

- `open`
- `pending`
- `addressed`

## Review states

- `draft`
- `pending`
- `waiting_for_response`
- `done`
