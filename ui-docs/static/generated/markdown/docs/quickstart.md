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
- the current repository root with `--root`

The review session is the structured review state Parley keeps locally:

- named reviews such as `my-review`
- line-anchored comment threads
- thread statuses: `open`, `pending`, `addressed`
- review state exists for compatibility, but TUI completion is thread-based

That matters because comments are not just free-form notes. Each thread is attached to a file path and line reference, and replies update the parent thread status.

The review session is the comment context. Switching reviews changes which comments and review state are visible, while the selected diff source stays as-is.

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

`--review` is required, and the review must already exist. Create it first with `review create`.

If your terminal or SSH session mishandles mouse reporting, disable mouse capture:

```bash
./target/release/parley tui --review my-review --no-mouse
```

The TUI never creates or guesses a startup review. Review context is explicit so comments are always written under the intended review.

## Open a specific commit or range

By default, the TUI opens the current working tree diff. You can also open historical revisions:

```bash
./target/release/parley tui --commit HEAD~2
./target/release/parley tui --base origin/trunk --head feature/my-branch
./target/release/parley tui --base v0.1.0
# everything after HEAD~2 (exclude that commit)
./target/release/parley tui --base HEAD~2 --head HEAD
# everything after and including HEAD~2
./target/release/parley tui --base HEAD~2^ --head HEAD
```

- `--commit <rev>` shows that commit against its first parent.
- `--base <rev>` defaults `head` to `HEAD`.
- `--base <rev> --head <rev>` shows an explicit range.
- Use `--base <rev> --head HEAD` to review everything after `<rev>`.
- Use `--base <rev>^ --head HEAD` to include `<rev>` itself in that cumulative range.

Refresh (`R`) keeps using the same source while the TUI session stays open, and AI prompt context follows that same diff source.

You can also switch to a specific commit from inside the TUI:

1. Press `Ctrl+k` to open the command palette.
2. Choose `Open Commit Picker`.
3. Type part of a commit message, short SHA, or full SHA to filter the recent commit list.
4. Use `↑/↓`, `j/k`, `PgUp/PgDn`, `Home`, or `End` to select a commit.
5. Press `Enter` to apply it.

The picker changes only Parley's active diff source. It does not run `git checkout` or modify the working tree.

To switch comment context without changing the diff:

1. Press `Ctrl+k` to open the command palette.
2. Choose `Open Review Picker`.
3. Filter by review name or state.
4. Press `Enter` to apply the selected review.

The TUI reloads comments from the selected review and keeps the current diff source.

To create a new review from inside the TUI, use `Ctrl+k` and `Create Review`. The new review becomes active immediately, and later comments are written to that review's directory. If the review picker has no matches for the typed name, pressing `Enter` opens the same create-review prompt with that name.

Current limitation:

- reopening the review later does not restore the revision source automatically; pass the same flags again.

## Review the current root directory

Use root mode when you want to review the repository as files, not as a git diff:

```bash
./target/release/parley tui --review my-review --root
```

`--review` is still required, and the review must already exist. Root mode does not create or guess a review.

Root mode includes tracked files and untracked files that are not ignored by gitignore rules. It skips `.git/`, `.parley/`, and `worktrees/`. Each file is displayed as context lines, so comments attach to the file's current line numbers.

Startup in root mode lazy-loads file content. The file tree appears first, load progress is shown while data hydrates, and individual files load when selected or opened from search. Root mode opens raw source by default. Press `D` / `Shift+d` or use command palette `Toggle Root JSON/Markdown Rendering` to switch `.json` files into pretty-printed JSON and Markdown files into readable rendered text rows.

## The core workflow

Think of the normal flow like this:

1. Open a diff.
2. Move to a changed line.
3. Create a thread on that line.
4. Reply until the issue is resolved.
5. Mark the thread addressed.
6. When each issue is resolved, mark its thread addressed.

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

- `x` runs AI refactor on the selected unresolved thread
- `X` runs AI reply on the selected thread
- `A` runs AI refactor across all eligible threads in the review

Typical pattern:

1. Leave a thread `open`.
2. Press `x` on that thread.
3. AI adds a reply to the thread.
4. Because AI is a different author, the thread becomes `pending`.
5. You inspect the code changes and either reopen the thread or mark it addressed.

Starting an AI run opens and follows the current file's AI logs so provider startup/config errors and stream output are visible.

### Customize AI task prompts

Parley always builds thread context from the selected comment, replies, target file, diff hunk, and referenced files. You can replace the final task instructions appended to that context with markdown files configured in `$HOME/.config/parley/config.toml`.

Use one shared prompt for all AI modes:

```toml
[ai]
prompt_path = "prompts/parley-ai.md"
```

Or use mode-specific prompts:

```toml
[ai]
reply_prompt_path = "prompts/parley-reply.md"
refactor_prompt_path = "prompts/parley-refactor.md"
```

Mode-specific paths take precedence over `prompt_path`. Relative paths are resolved from the repository root where Parley is started. If a configured prompt file cannot be read, the AI session fails before invoking the provider.

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
- `/<text>`: search within the current file
- `Ctrl+g`: open codebase search popup
- File and codebase search use `rg` when available and fall back to `grep`
- Codebase search respects gitignore rules and updates results while you type
- `Enter` or mouse click on a result opens the matched file and line
- `n/p`: next or previous search hit

### Threads

- `m` or `c`: create thread on selected line
- `v` or `V`: start or clear visual line selection
- With visual line selection active, move to the end of the range and press `m` or `c` to open the comment box at the bottom of that range
- `r`: reply to selected thread
- `N/P`: jump next or previous thread
- `[/]`: select previous or next thread in current file
- `Ctrl+t`: open the global thread selector
- `e`: toggle selected thread expansion
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
- Long comment drafts wrap inside the editor, preserving whole words when possible

### Review state

- `s`: set review `open`
- `w`: set review `under_review`

Review state mostly follows thread state:

- any `open` thread means the review is `open`
- no `open` threads means the review is `under_review`
- thread `addressed` is the completion signal used in the TUI

### AI and tools

- `x`: AI refactor selected thread
- `X`: AI reply selected thread
- `A`: AI refactor full review
- `i`: cycle AI provider (`codex`, `claude`, `opencode`, `pi`)
- `I`: toggle AI transport (`acp` or `cli`) for providers that support both
- `K`: cancel current AI run
- `H`: toggle per-file AI logs popup
- `L`: toggle the global AI activity pane
- AI activity `Enter`: jump to the selected file/session logs
- `Ctrl+k`: open command palette
- Command palette `Search Codebase`: open live repository search
- Command palette `Show AI Activity`: open the global AI session activity pane
- Command palette `Toggle AI Transport`: switch between ACP and CLI for the active provider
- Command palette `Open Thread Selector`: search and jump across all review threads
- Command palette `Show Git File Heatmap`: scan git history on demand and show file hotspots
- Command palette `Open Commit Picker`: switch the active diff source to a recent commit
- Command palette `Open Review Picker`: switch the active review context
- Command palette `Create Review`: create a new review context and switch to it
- `M`: open the git file heatmap
- Heatmap `s`: cycle sort by churn, added, removed, commits, net growth, net shrink, volatility, or path
- Heatmap `S`: reverse the active heatmap sort
- `Ctrl+f`: focus files filter input
- `?`: open in-app docs/help overlay

By default, agent providers use persistent transports instead of spawning a one-shot CLI prompt for every thread. OpenCode uses ACP with `opencode acp`, Codex uses `codex-acp`, Claude uses `claude-agent-acp`, and Pi uses `pi --mode rpc --no-session`. Pi is RPC, not ACP. Set a provider's `transport = "cli"` in `$HOME/.config/parley/config.toml` only when you explicitly need the old one-shot behavior. If ACP is configured with a non-ACP command such as `codex exec` or `opencode run`, Parley fails fast and shows the config error in the AI logs.

The default `$HOME/.config/parley/config.toml` AI shape is:

```toml
[ai]
timeout_seconds = 120
default_provider = "opencode"
default_transport = "acp"

[ai.codex]
transport = "acp"
client = "codex-acp"
args = []

[ai.claude]
transport = "acp"
client = "claude-agent-acp"
args = []

[ai.opencode]
transport = "acp"
client = "opencode"
args = ["acp"]
model_arg = "-m"

[ai.pi]
transport = "pi_rpc"
client = "pi"
args = ["--mode", "rpc", "--no-session"]
```

For one-shot CLI mode, configure the provider explicitly:

```toml
[ai.codex]
transport = "cli"
client = "codex"
args = ["exec"]
```

The TUI can also switch transport at runtime with `I`. The active selection is stored as `ai.default_transport`, which accepts only the generic `acp` and `cli` choices. Parley uses built-in CLI profiles for `codex`, `claude`, and `opencode` when that value is `cli`. Pi keeps using provider-specific `pi_rpc`.

Parley stores AI output as per-file session logs in memory while the TUI is open. Starting an AI run opens/follows the current file logs. `H` shows transcripts for the current file, and `L` shows a global activity index for running and recent sessions. Review comments are separate durable state; AI output is added to a thread only when the AI review flow persists a reply.

The thread selector is separate from the per-file thread navigator. `Ctrl+t` searches all threads in the active review and jumps to the selected file/thread with `Enter`. In root mode, comments whose original anchor text no longer matches are still displayed at their stored line reference so pending threads do not disappear after refactors.

### Split diff layout

- `Ctrl+v`: toggle split view
- Command palette `Toggle Split View`: toggle split view from the command list
- `S`: toggle side-by-side diff layout
- `Tab`: switch active diff pane
- Added and removed lines use tinted backgrounds to make changed regions easier to scan

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
./target/release/parley tui --review parser-cleanup --base origin/trunk --head feature/parser-cleanup
```

That lets you keep one named review session while pointing the TUI at an explicit base/head diff.

## Config

Parley reads configuration from `$HOME/.config/parley/config.toml` and stores review-owned state under `.parley/reviews/<review-name>/`. If the user config file does not exist, Parley still reads the legacy `.parley/config.toml` path for existing checkouts.

Each review directory contains:

```text
review.json
logs/tui.log
```

All comments, replies, thread status, review status, and TUI logs for that review stay under this directory.

Older flat review files in `.parley/reviews/<review-name>.json` are still loaded.

By default, Parley ignores its own `.parley/` files when building the review diff so that review metadata and logs do not show up in the file list.

To include `.parley/` in the diff again, set:

```toml
ignore_parley_dir = false
```

### Which status to set before AI

- For review-wide `refactor` (`A`): target threads must be `open` or `pending`.
- For selected-thread AI (`x` or `X`): the selected thread is sent unless it is `addressed`.

## Refresh after edits

Inside TUI:

```text
R
```

This reloads review metadata and the active diff source.

- For the normal workflow, that means the current working tree diff.
- If you opened the TUI with `--commit` or `--base` / `--head`, refresh keeps using that same historical source.
