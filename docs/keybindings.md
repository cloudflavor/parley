# Keybindings

## Navigation

- `h/l`: previous/next file
- `j/k`: down/up line
- `PgUp/PgDn`: page scroll
- `Ctrl+u/Ctrl+d`: half-page scroll
- `g/G`: top/bottom
- `zz`: center active line

## Search and jump

- `:<line>`: go to line
- `/` or `Ctrl+g`: open codebase search popup (`rg`, falling back to `grep`)
- Code search `type`: update results live
- Code search `Enter`: open selected file and line
- Code search `↑/↓`, `j/k`, `PgUp/PgDn`, `Home`, `End`: move selected result
- Code search mouse: click a result to open that file and line
- Code search `Esc`: close search
- `n/p`: next/previous in-diff search match

## Threads

- `m` or `c`: create thread on selected line
- `v` or `V`: start or clear visual line selection for a range comment
- With visual line selection active, move with `j/k` or arrows, then press `m` or `c` to open the comment box at the bottom of the selected range
- `r`: reply to selected thread
- `N/P`: next/previous thread
- `[/]`: previous/next selected thread in file
- `e`: toggle selected thread expansion
- `Shift+E`: cycle thread density
- `a`: mark addressed
- `o`: mark open
- `f`: force-address selected thread
- `u`: re-anchor selected thread to the currently selected diff line

### File references inside the comment box

- Type `@` in the comment or reply box to open the file reference picker.
- `↑/↓` or `PgUp/PgDn`: move through file matches
- `Enter` or `Tab`: accept the selected file and enter line-picker mode
- While line-picker mode is active, Parley opens that file in the current diff pane and tells you to select a diff line in the editor itself.
- `↑/↓`, `j/k`, `PgUp/PgDn`, `g/G`: move to the target diff line
- `Enter` or `Tab`: insert `@path:line` for the currently selected line
- Mouse: click a diff line while line-picker mode is active to insert that line immediately
- After inserting the reference, Parley returns to the file and diff line where you started writing the draft.
- `Esc`: cancel the picker; if the file is already inserted, it leaves the bare `@path` in place

### Comment editor word motions

- `Alt+b`: move backward one whitespace-delimited word in the draft
- `Alt+d`: delete forward through the next whitespace-delimited word in the draft
- Long comments wrap inside the editor, preserving whole words when possible

## Review state

- `s`: set review state `open`
- `w`: set review state `under_review`
- `d`: set review state `done` (guarded)
- `Shift+D`: force set `done`

## AI

- `x`: AI refactor selected thread
- `X`: AI reply selected thread
- `A`: AI refactor review
- `K`: cancel active AI run
- `H`: toggle per-file AI logs popup
- `L`: open review TUI log file in `less`

## Layout and tools

- `?`: open help docs
- `Ctrl+k`: command palette
- Command palette `Search Codebase`: open live repository search
- Command palette `Toggle Active File Group`: collapse or expand the active file group
- Command palette `Collapse All File Groups`: collapse every file group visible under the current filter
- Command palette `Open Commit Picker`: open recent commits, filter by message or SHA, and apply the selected commit as the active diff source
- Command palette `Open Review Picker`: open reviews, filter by name or state, and apply the selected review as the active comment context
- Command palette `Create Review`: create a new review context and switch to it
- `M` or command palette `Show Git File Heatmap`: scan full git history on demand and show file hotspots
- Heatmap `s`: cycle sort (`churn`, `added`, `removed`, `commits`, `net-growth`, `net-shrink`, `volatility`, `path`)
- Heatmap `S`: reverse sort direction
- Heatmap color follows the active sort metric
- `Ctrl+f`: file filter input
- `Shift+U`: edit user name
- `Ctrl+v`: toggle split diff
- Command palette `Toggle Split Diff View`: toggle split diff without using the visual-selection key
- `S`: toggle side-by-side diff
- `Tab`: switch active diff pane
- `</>`: resize files pane
- `Enter`: collapse or expand the active file group
- `Shift+C`: collapse all visible file groups
- Mouse: click a file group header to collapse or expand it
- `b`: toggle thread navigator

## Help pane

- `Tab` / `Shift+Tab` or `h/l`: switch help doc tab
- `1-9`: direct tab select
- `j/k`, `PgUp/PgDn`, `g/G`: scroll help content
- `</>`: zoom help pane
- `Esc` or `?`: close help pane
