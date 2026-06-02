# Keybindings

## Navigation

- `q`: quit
- `h/l` or `Left/Right`: previous/next file
- `j/k` or `Up/Down`: down/up line
- `PgUp/PgDn`: page scroll
- `Ctrl+u/Ctrl+d`: half-page scroll
- `g`: top
- `Shift+G`: bottom
- `z`: toggle content fullscreen
- `Shift+Z`: center active line in viewport
- `R`: refresh review and diff/root source

## Search and jump

- `:<line>`: go to line
- `/<text>`: search within the current file (`rg`, falling back to `grep`)
- `n/p`: next/previous in-diff search match
- `Ctrl+g`: open codebase search popup (`rg`, falling back to `grep`)
- Code search `type`: update results live
- Code search `Enter`: open selected file and line
- Code search `↑/↓`, `j/k`, `PgUp/PgDn`, `Home`, `End`: move selected result
- Code search `Ctrl+a/Ctrl+e`: move cursor to start/end of query
- Code search mouse: click a result to open that file and line
- Code search `Esc`: close search

## Threads

- `m` or `c`: create thread on selected line
- `v`: start visual line selection for a range comment
- `Shift+V`: clear visual line selection
- With visual line selection active, move with `j/k` or arrows, then press `m` or `c` to open the comment box at the bottom of the selected range
- `r`: reply to selected thread
- `N/P`: next/previous thread
- `[/]`: previous/next thread in current file
- `Ctrl+t`: open global thread selector
- Thread selector `Enter`: jump to selected thread and file
- Thread selector `type`: filter by file, status, id, line, or body preview
- Thread selector `Ctrl+a/Ctrl+e`: move cursor to start/end of filter query
- Thread selector `Esc` or `Ctrl+t`: close selector
- `e`: toggle selected thread expansion
- `Ctrl+e`: toggle selected thread anchor context expansion (collapsed by default)
- `a`: mark addressed
- `o`: mark open
- `f`: force-address selected thread
- `u`: re-anchor selected thread to the currently selected diff line

### File references inside the comment box

- Type `@` in the comment or reply box to open the file reference picker.
- `↑/↓` or `PgUp/PgDn`: move through file matches
- `Enter` or `Tab`: accept the selected file and enter line-picker mode
- While line-picker mode is active, Parley opens that file in the current diff pane and tells you to select a diff line in the editor itself.
- `↑/↓`, `j/k`, `PgUp/PgDn`, `g/Shift+G`: move to the target diff line
- `Enter` or `Tab`: insert `@path:line` for the currently selected line
- Mouse: click a diff line while line-picker mode is active to insert that line immediately
- After inserting the reference, Parley returns to the file and diff line where you started writing the draft.
- `Esc`: cancel the picker; if the file is already inserted, it leaves the bare `@path` in place

### Comment editor

- `Esc`: close comment box
- `Ctrl+s`: save comment or reply
- `Ctrl+p`: toggle markdown preview in comment editor
- Arrow keys or `Ctrl+p/n/b/f`: move cursor (up/down/left/right)
- `Home/End`: move to start/end of line
- `Ctrl+a/Ctrl+e`: move to start/end of line
- `Ctrl+k`: kill to end of line
- `Enter`: insert newline
- `Tab`: insert 4 spaces
- `Backspace/Delete`: delete character
- `Alt+b`: move backward one whitespace-delimited word in the draft
- `Alt+d`: delete forward through the next whitespace-delimited word in the draft
- Long comments wrap inside the editor, preserving whole words when possible

## Review state

- `s`: set review state `open`
- `w`: set review state `under_review`

## AI

- `x`: AI refactor selected thread
- `X`: AI reply selected thread
- `A`: AI refactor review
- `i`: cycle AI provider (`codex`, `claude`, `opencode`, `pi`)
- `I`: toggle AI transport between ACP and CLI for providers that support both
- `K`: cancel active AI run
- `H`: toggle per-file AI logs popup
- `L`: toggle global AI activity pane
- Starting an AI run opens/follows the current file's AI logs so provider startup errors and stream output are visible.
- AI activity `Enter`: jump to the selected file/session logs
- AI activity `j/k`, `PgUp/PgDn`, `Home/End`: select a session
- AI activity `Esc` or `L`: close activity pane
- AI activity `O/o`: open AI log in external pager
- AI progress popup `PgUp/PgDn`: scroll AI stream
- AI progress popup `Home/End`: jump to beginning/latest of AI stream
- AI progress popup `O/o`: open AI log in external pager

## Layout and tools

- `?`: open help docs
- `Ctrl+k`: command palette
- Command palette `Ctrl+a/Ctrl+e`: move cursor to start/end of query
- Command palette `Search Codebase`: open live repository search
- Command palette `Show AI Activity`: open the global AI session activity pane
- Command palette `Toggle AI Transport`: switch between ACP and CLI for the active provider
- Command palette `Toggle Active File Group`: collapse or expand the active file group
- Command palette `Collapse All File Groups`: collapse every file group visible under the current filter
- Command palette `Open Commit Picker`: open recent commits, filter by message or SHA, and apply the selected commit as the active diff source
- Command palette `Open Branch Picker`: switch git branch from a filtered list
- Command palette `Open Review Picker`: open reviews, filter by name or state, and apply the selected review as the active comment context
- Command palette `Open Thread Selector`: search and jump across all review threads
- Command palette `Create Review`: create a new review context and switch to it
- `M` or command palette `Show Git File Heatmap`: scan full git history on demand and show file hotspots
- Heatmap `j/k` or `↑/↓`: scroll heatmap
- Heatmap `PgUp/PgDn`: page scroll heatmap
- Heatmap `g/Shift+G` or `Home/End`: jump to top/bottom
- Heatmap `s`: cycle sort (`churn`, `added`, `removed`, `commits`, `net-growth`, `net-shrink`, `volatility`, `path`)
- Heatmap `Shift+S`: reverse sort direction
- Heatmap `Esc` or `M`: close heatmap
- Heatmap color follows the active sort metric
- `Ctrl+f`: file filter input
- `Esc` or `Enter` or `Ctrl+f`: exit file filter
- `Shift+F`: cycle file filter mode
- `Shift+O`: open file viewer
- `Shift+U`: edit user name
- `t`: open theme picker
- `T`: toggle light/dark theme variant
- `Ctrl+v`: toggle split view
- Command palette `Toggle Split View`: toggle split view without using the visual-selection key
- `S`: toggle side-by-side diff
- `Tab`: switch active diff pane (when split view is enabled)
- `</>`: resize files pane
- `Enter`: collapse or expand the active file group
- `Shift+C`: collapse all visible file groups
- Mouse: click a file group header to collapse or expand it
- `b`: toggle thread navigator
- `Ctrl+b`: open branch picker (switch git branch)
- `Ctrl+w`: open worktree picker (switch git worktree)
- `Ctrl+o`: open commit picker (apply a commit as the active diff source)
- `Ctrl+z`: suspend Parley (run `fg` to resume)
- `Ctrl+t`: open thread selector (also closes it when open)

## Root file rendering

- `D` / `Shift+d`: toggle rendered document view in `--root` mode.
- Rendered document view is off by default; root mode opens files as raw source rows.
- With rendered document view enabled, `.json` files are shown as pretty-printed JSON.
- With rendered document view enabled, `.md`, `.markdown`, `.mdown`, and `.mkd` files are rendered as readable Markdown text rows.
- `R`: refresh the root source if file content changed while Parley is open.
- Command palette `Toggle Root JSON/Markdown Rendering`: same as `D` / `Shift+d`.
- Command palette search terms `json`, `markdown`, `pretty`, or `render` surface the toggle.

## Help pane

- `Tab` / `Shift+Tab` or `h/l` or `Left/Right`: switch help doc tab
- `1-9`: direct tab select
- `j/k` or `↑/Down`: scroll help content
- `PgUp/PgDn`: page scroll help content
- `g/Shift+G` or `Home/End`: jump to top/bottom of help content
- `</>`: zoom help pane
- `Esc` or `?`: close help pane
- Mouse scroll: scroll help content
