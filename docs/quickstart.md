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
