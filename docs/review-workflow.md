# Review Workflow

## 1. Create threads on concrete lines

Use line-level comments for actionable feedback. Keep one issue per thread so resolution is obvious.

## 2. Drive thread state deliberately

- Use `open` when work is needed.
- Use `pending` when waiting on response or follow-up.
- Use `addressed` once the original concern is resolved.

## 3. Keep review state aligned with thread state

Use review state transitions to reflect real project status:

- `pending`: active work
- `waiting_for_response`: blocked on counterpart response
- `done`: all threads resolved

## 4. Add AI output as replies, not as implicit state changes

AI runs can produce candidate responses/refactors, but thread/review states should still be explicitly managed by the reviewer/author.

## 5. Refresh often

After code edits or automation runs, refresh in TUI so thread anchors and diff context stay current.
