# Review Workflow

## 1. Thread model (authoritative)

Each thread is a single `LineComment` with:

- anchor (`file_path`, `old_line`, `new_line`, `side`)
- thread `status` (`open`, `pending`, `addressed`)
- thread `author` (original commenter)
- ordered replies

Replies do not have thread status. A reply event updates the parent thread status.

## 2. Thread status transitions

### Automatic transitions

- New comment created -> `open`
- Reply by original thread author -> `open`
- Reply by different author (including AI in normal flows) -> `pending`

### Manual transitions

- Mark open: original thread author -> `open`
- Mark addressed: original thread author -> `addressed`
- Force mark addressed: force path (no author gate) -> `addressed`

## 3. Review state reconciliation

Review state is reconciled from thread statuses:

- if any thread is `open`, review is `open`
- else if no thread is `open`, review is `under_review`
- `done` is explicit and guarded

`done` guard:

- normal set to `done` fails when unresolved threads (`open` or `pending`) exist
- force done bypasses this check
- if new unresolved activity appears after `done`, review auto-reopens to `open`

## 4. Threading practice

Use line-level comments for actionable feedback. Keep one issue per thread so resolution is obvious.

## 5. Drive thread state deliberately

- Use `open` when code changes are required.
- Use `pending` when a reply is waiting on counterpart action.
- Use `addressed` when the original reviewer confirms resolution.

## 6. AI eligibility matrix (what status to use before sending to AI)

Global precondition:

- review must not be `done` (AI session is skipped otherwise)

### Mode = `refactor`

- No explicit `comment_ids` (auto-target):
  - processed: `open`
  - skipped: `pending`, `addressed`
- Explicit `comment_ids`:
  - processed: `open`
  - skipped: `pending`, `addressed`

What this means:

- set thread to `open` before running AI refactor
- `pending` or `addressed` threads will not be processed in refactor mode

### Mode = `reply`

- No explicit `comment_ids` (auto-target):
  - processed: `open`, `pending`
  - skipped: `addressed`
- Explicit `comment_ids`:
  - processed: any selected status (including `addressed`)
  - skipped by status filter: none

What this means:

- for normal reply runs, use `open` or `pending`
- explicit thread targeting can still send an `addressed` thread to AI reply mode

## 7. Post-AI behavior

- AI output is persisted as a reply in the target thread.
- In typical human-authored threads, this sets thread status to `pending`.
- Review state then reconciles based on resulting thread statuses.

## 8. Refresh after code edits

After code edits or automation runs, refresh in TUI so thread anchors and diff context stay current.
