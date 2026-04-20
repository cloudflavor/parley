# Quickstart

## Build

```bash
cargo build --release
```

## What Parley is actually doing

Parley separates two things:

- a **diff source** you are reviewing
- a **review session** stored under `.parley/`

The diff source is the code you see in the TUI:

- the current working tree by default
- a specific commit with `--commit`
- a base/head range with `--base` and `--head`

The review session is the structured review state Parley keeps locally:

- named reviews such as `my-review`
- line-anchored comment threads
- thread statuses: `open`, `pending`, `addressed`
- review states: `open`, `under_review`, `done`

That matters because comments are not just free-form notes. Each thread is attached to a file path and line reference, and replies update the parent thread status.

## Create a review session

```bash
./target/release/parley review create my-review
./target/release/parley review start my-review
```

`review create` creates the local session. `review start` moves it into `under_review`, but the moment you add an `open` thread, the review state automatically becomes `open` again.

## Open the TUI on your current changes

```bash
./target/release/parley tui --review my-review
```

If your terminal or SSH session mishandles mouse reporting, disable mouse capture:

```bash
./target/release/parley tui --review my-review --no-mouse
```

If only one review exists, Parley can resolve that review automatically, but using `--review` is the clearest form.

## Open a specific commit or range

By default, the TUI opens the current working tree diff. You can also open historical revisions:

```bash
./target/release/parley tui --commit HEAD~2
./target/release/parley tui --base main --head feature/my-branch
./target/release/parley tui --base v0.1.0
```

- `--commit <rev>` shows that commit against its first parent.
- `--base <rev>` defaults `head` to `HEAD`.
- `--base <rev> --head <rev>` shows an explicit range.

Refresh (`R`) keeps using the same source while the TUI session stays open, and AI prompt context follows that same diff source.

Current limitation:

- reopening the review later does not restore the revision source automatically; pass the same flags again.

## The core workflow

Think of the normal flow like this:

1. Open a diff.
2. Move to a changed line.
3. Create a thread on that line.
4. Reply until the issue is resolved.
5. Mark the thread addressed.
6. When nothing unresolved remains, move the review to `done`.

### Example: review one issue end to end

Say you open a diff and find a risky change in `src/lib.rs`.

1. Move to the line with `j/k`, `PgUp/PgDn`, or search.
2. Press `m` or `c` to open a draft on that line.
3. Write a comment such as:

```text
This branch drops the error context. Can we keep the original cause here?
```

4. Save with `Ctrl+s`.

That creates a thread anchored to the selected diff line, and the thread starts as `open`.

If someone else replies, for example:

```text
I pushed a fix and kept the original error chain.
```

the thread becomes `pending`.

If the original commenter replies again, the thread goes back to `open`.

When the original commenter is satisfied, they mark it `addressed`.

Only the original commenter can normally change a thread to `open`, `pending`, or `addressed`. That is enforced by the review model.

### Example: use AI on a thread

- `x` runs AI refactor on the selected `open` thread
- `X` runs AI reply on the selected thread
- `A` runs AI refactor across all eligible threads in the review

Typical pattern:

1. Leave a thread `open`.
2. Press `x` on that thread.
3. AI adds a reply to the thread.
4. Because AI is a different author, the thread becomes `pending`.
5. You inspect the code changes and either reopen the thread or mark it addressed.

If the review is already `done`, AI runs are skipped.

## Core controls

### Navigation

- `h/l`: previous or next file
- `j/k`: move line cursor down or up
- `PgUp/PgDn`: page scroll
- `Ctrl+u/Ctrl+d`: half-page scroll
- `g/G`: jump to first or last line
- `zz`: center active line

### Search and jump

- `:<line>`: go to line
- `/query`: set diff search query
- `n/p`: next or previous search hit

### Threads

- `m` or `c`: create thread on selected line
- `r`: reply to selected thread
- `N/P`: jump next or previous thread
- `[/]`: select previous or next thread in current file
- `e`: toggle selected thread expansion
- `Shift+E`: cycle thread density (`compact`/`expanded`)
- `a/o/f`: addressed/open/force-address selected thread

### File references in comments

- Type `@` inside the comment or reply box to open file matching.
- Use `Enter` or `Tab` on a file match to open that file in the active diff pane and switch into line-picker mode.
- The comment editor explicitly tells you to select a diff line before the reference is confirmed.
- In line-picker mode, use `↑/↓`, `j/k`, `PgUp/PgDn`, or `g/G` to move the diff cursor, then `Enter` or `Tab` to insert `@path:line`.
- If mouse support is enabled, clicking a diff line during line-picker mode inserts that line immediately.
- After inserting the reference, Parley restores the file, pane, and diff line where the draft started.
- `Esc` cancels the line picker and leaves the bare `@path` reference in the comment buffer.

### Comment editor word motions

- `Alt+b`: move backward one whitespace-delimited word in the draft
- `Alt+d`: delete forward through the next whitespace-delimited word in the draft

### Review state

- `s`: set review `open`
- `w`: set review `under_review`
- `d`: set review `done` (blocked if unresolved threads exist)
- `Shift+D`: force set review `done`

Review state mostly follows thread state:

- any `open` thread means the review is `open`
- no `open` threads means the review is `under_review`
- `done` is explicit and guarded

`done` is blocked while unresolved threads remain. In practice, that means both `open` and `pending` threads prevent a normal transition to `done`.

### AI and tools

- `x`: AI refactor selected thread
- `X`: AI reply selected thread
- `A`: AI refactor full review
- `K`: cancel current AI run
- `H`: toggle AI stream popup
- `L`: open log file in `less`
- `Ctrl+k`: open command palette
- `Ctrl+f`: focus files filter input
- `?`: open in-app docs/help overlay

## A realistic first session

```bash
./target/release/parley review create parser-cleanup
./target/release/parley review start parser-cleanup
./target/release/parley tui --review parser-cleanup
```

Inside the TUI:

- move through changed files with `h/l`
- move inside the diff with `j/k`
- press `c` on a changed line to leave a thread
- press `r` on that thread to reply later
- press `R` after editing code in another terminal
- press `a` when the original reviewer considers the issue resolved
- press `d` when the review is actually complete

For a historical review:

```bash
./target/release/parley tui --review parser-cleanup --base main --head feature/parser-cleanup
```

That lets you keep one named review session while pointing the TUI at an explicit base/head diff.

## Config

Parley stores its local state in `.parley/` and reads configuration from `.parley/config.toml`.

By default, Parley ignores its own `.parley/` files when building the review diff so that review metadata and logs do not show up in the file list.

To include `.parley/` in the diff again, set:

```toml
ignore_parley_dir = false
```

### Which status to set before AI

- For `refactor` (`x` or `A`): thread must be `open`.
- For `reply` (`X`): thread should be `open` or `pending`.
- If review is `done`, AI runs are skipped.
- Use explicit thread selection from MCP if you need reply mode on an `addressed` thread.

## Refresh after edits

Inside TUI:

```text
R
```

This reloads review metadata and the active diff source.

- For the normal workflow, that means the current working tree diff.
- If you opened the TUI with `--commit` or `--base` / `--head`, refresh keeps using that same historical source.
