# Quickstart

## Build

```bash
cargo build --release
```

## Create and start a review

```bash
./target/release/parley review create my-review
./target/release/parley review start my-review
```

## Open the TUI

```bash
./target/release/parley tui --review my-review
```

If your terminal or SSH session mishandles mouse reporting, disable mouse capture:

```bash
./target/release/parley tui --review my-review --no-mouse
```

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

This reloads review metadata and current git diff.
