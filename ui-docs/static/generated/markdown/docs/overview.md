# Parley Overview

Parley is a terminal-first review tool for local git diffs, optimized for iterative AI-assisted code review.

Review discussions are anchored to concrete diff lines, and both thread state and review state are explicit.

## Core model

- **Threaded line review**: each comment thread is anchored to file path + line reference.
- **Thread lifecycle**: `open`, `pending`, `addressed`.
- **Review lifecycle**: `open`, `under_review`, `done`.
- **Keyboard-first workflow**: full navigation and review operations without leaving the terminal.
- **Optional AI automation**: run AI thread replies/refactors while keeping state transitions human-controlled.

## How to think about the app

Parley is not just a diff viewer and not just a notes file.

It combines:

- a diff source: working tree, commit, or range
- a local review session: named review metadata stored under `.parley/`

Each review is its own context. Switching reviews changes the comment threads, replies, and review status shown in the TUI; it does not change the active diff source.

That review session tracks threads as structured objects with:

- an anchor line in the diff
- an original author
- a thread status
- ordered replies

This is why status changes are opinionated:

- new thread -> `open`
- reply from the original commenter -> `open`
- reply from anyone else, including AI -> `pending`
- original commenter marks resolution -> `addressed`

The review state is then derived from the unresolved thread set until you explicitly set it to `done`.

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

The TUI requires an explicit existing review. Create the review first with `parley review create <name>`, then open it with `parley tui --review <name>`.

If you are running over SSH and your terminal client does not play well with mouse reporting, start the TUI with `--no-mouse`.

## Revision sources

By default, Parley reviews the current working tree diff against `HEAD`.

You can also open historical diffs directly in the TUI:

```bash
parley tui --commit HEAD~2
parley tui --base origin/trunk --head feature/my-branch
parley tui --base v0.1.0
# everything after HEAD~2 (exclude that commit)
parley tui --base HEAD~2 --head HEAD
# everything after and including HEAD~2
parley tui --base HEAD~2^ --head HEAD
```

- `--commit <rev>` reviews that commit against its first parent.
- `--base <rev>` reviews `<rev>..HEAD`.
- `--base <rev> --head <rev>` reviews an explicit tree-to-tree range.
- Use `--base <rev> --head HEAD` to review everything after `<rev>`.
- Use `--base <rev>^ --head HEAD` to include `<rev>` itself in that cumulative range.

AI sessions and TUI refresh use the same selected revision source, so they stay aligned with the diff you opened.

From inside the TUI, use `Ctrl+k` to open the command palette, choose `Open Commit Picker`, then search by commit message or SHA. `Enter` switches the active diff source to the selected commit, and `Esc` closes the picker without changing the current diff.

Use `Ctrl+k` and `Open Review Picker` to switch the active review context. The picker filters by review name or state and shows each review's thread counts. Applying a review reloads the review-owned comments while keeping the current diff source.

Use `Ctrl+k` and `Create Review` to create a new review from inside the TUI and switch to it immediately. Entering a name in the review picker that has no matches also opens the create-review prompt.

Current limitation:

- the selected revision source is not persisted into the review session yet, so reopening the review later still requires passing the same CLI flags again.

## File references in comment drafts

Inside the inline comment or reply editor, `@` opens file matching against the current diff. Accepting a file switches the active diff pane to that file and enters a line-picker mode so you can move to or click the exact diff line before Parley inserts `@path:line` into the draft.

The editor itself calls out that a line still needs to be selected, and once the reference is inserted Parley restores the pane, file, and line where the draft originally started.

This keeps file references understandable to humans reading the thread instead of relying on a bare path plus a manually typed line number.

Inside that same draft editor, `Alt+b` moves backward by the previous whitespace-delimited word and `Alt+d` deletes forward through the next one.

## Local state and diff filtering

Parley stores config under `.parley/` and review-owned data under normalized review directories:

```text
.parley/
  config.toml
  reviews/
    <review-name>/
      review.json
      logs/
        tui.log
```

Comments, replies, thread state, review state, and TUI logs belong to that review directory.

Older flat files such as `.parley/reviews/<review-name>.json` are still readable.

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
