# Parley Overview

Parley is a terminal-first review tool for local git diffs, optimized for iterative AI-assisted code review.

Install with:

### Homebrew (macOS and Linux)

```bash
brew tap cloudflavor/tap
brew install cloudflavor/tap/parley
```

### Cargo

```bash
cargo install parley-cli
```

Review discussions are anchored to concrete diff lines, and thread state is explicit.

## Core model

- **Threaded line review**: each comment thread is anchored to file path + line reference.
- **Thread lifecycle**: `open`, `pending`, `addressed`.
- **Review lifecycle**: TUI progress is based on per-thread status.
- **Keyboard-first workflow**: full navigation and review operations without leaving the terminal.
- **Optional AI automation**: run AI thread replies/refactors while keeping state transitions human-controlled.

## How to think about the app

Parley is not just a diff viewer and not just a notes file.

It combines:

- a diff source: working tree, commit, or range
- a local review session: named review metadata stored in the active Parley store

Each review is its own context. Switching reviews changes the comment threads, replies, and review status shown in the TUI; it does not change the active diff source.

Parley uses a repository-local `.parley/` directory only when that directory already exists. Otherwise it stores the current repository's state under `$HOME/.config/parley/repos/<repo-name>-<hash>/`. Run `parley config use-local` to explicitly create `.parley/` for a repository that should keep Parley state inside the project. Run `parley config path` to print the active store path.

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

The TUI treats individual thread status as the source of review progress.

## Thread model at a glance

- A thread starts as `open` when a comment is created.
- Replies do not carry their own status; they update the parent thread status.
- When the original thread author replies, the thread becomes `open`.
- When a different author replies (including AI in normal flows), the thread becomes `pending`.
- `addressed` is explicit: the original thread author marks the thread resolved.

## Review model at a glance

- `open`: at least one thread is `open`.
- `under_review`: no `open` threads remain.
- Thread `addressed` is the completion signal.

## AI eligibility summary

- `reply` mode targets `open` + `pending` threads by default.
- `refactor` mode targets `open` + `pending` threads by default.
- Explicit selected-thread AI actions target the selected thread regardless of status.

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

Use `Ctrl+k` and `Create Review` to create a new review from inside the TUI and switch to it immediately. Entering a name in the review picker that has no matches also opens the create-review prompt with that name.

Use `Ctrl+k` and `Open Branch Picker` to switch git branch from a filtered list.

Use `Ctrl+k` and `Open Worktree Picker` to switch git worktree from a filtered list. You can also start Parley against a specific worktree from the CLI with `--worktree <name|path>`.

Current limitation:

- the selected revision source is not persisted into the review session yet, so reopening the review later still requires passing the same CLI flags again.

## Root directory reviews

Use `--root` when you want to review files in the current repository root instead of a git diff:

```bash
parley tui --review my-review --root
```

`--review` is still required, and the review must already exist. Create it first with `parley review create <name>`.

Root mode loads tracked files plus untracked files that are not ignored by git. It skips `.git/`, `.parley/`, and `worktrees/`. Files are shown as context lines, so comments attach to the current file line numbers instead of added or removed diff lines.

Root mode is lazy-loaded for startup performance. The TUI builds the file tree first, shows load progress while file data hydrates, and loads file content when the file is selected or opened from search. Root mode opens raw source by default. Press `D` / `Shift+d` or use command palette `Toggle Root JSON/Markdown Rendering` to switch JSON files into pretty-printed display and Markdown files into readable rendered text rows.

## Finding code and hotspots

Current-file search is available from `/` and uses `rg` when available with a `grep` fallback. Codebase search is available from `Ctrl+g` or command palette `Search Codebase`; results update while typing, show the match count and search engine, and `Enter` or mouse click opens the selected file at the matched line.

The git file heatmap is available from `M` or command palette `Show Git File Heatmap`. It scans git history only when requested, not at startup, and renders per-file hotspots as colored cells.

Heatmap sort modes:

- `churn`: added plus removed lines
- `added`: total lines added
- `removed`: total lines removed
- `commits`: commits touching the file
- `net-growth`: added minus removed lines
- `net-shrink`: removed minus added lines
- `volatility`: churn per touching commit
- `path`: path name

Press `s` in the heatmap to cycle sort mode and `Shift+S` to reverse sort direction. Heatmap color intensity follows the active sort metric, so the strongest color always means the file is hottest for the current sort.

## File references in comment drafts

Inside the inline comment or reply editor, `@` opens file matching against the current diff. Accepting a file switches the active diff pane to that file and enters a line-picker mode so you can move to or click the exact diff line before Parley inserts `@path:line` into the draft.

The editor itself calls out that a line still needs to be selected, and once the reference is inserted Parley restores the pane, file, and line where the draft originally started.

This keeps file references understandable to humans reading the thread instead of relying on a bare path plus a manually typed line number.

Inside that same draft editor, `Alt+b` moves backward by the previous whitespace-delimited word and `Alt+d` deletes forward through the next one.

Comment drafts wrap inside the editor instead of extending as one long terminal line. Wrapped display preserves words when possible, so long comments remain readable before saving. Markdown preview is available in the comment editor with `Ctrl+p`.

## Split diffs, logs, and AI sessions

Use `Ctrl+v` or command palette `Toggle Split View` to toggle split view, `S` to toggle side-by-side diff layout, and `Tab` to switch the active pane. Added and removed lines use tinted backgrounds so large diffs are easier to scan.

Use `v` or `Shift+V` for visual line selection before creating a range comment. After selecting a range, `m` or `c` opens the comment box at the bottom of the selection. Saved range comments keep the covered lines highlighted and send the selected line range to AI prompt context.

Use `Ctrl+t` or command palette `Open Thread Selector` to search all threads in the active review by file, status, id, line reference, or body preview. Selecting a thread jumps to its file and focuses the thread. In root mode, stale or detached comments are still shown at their stored line reference when the original anchor text no longer matches current file content.

AI task output is tracked as file-scoped sessions in the TUI. Starting an AI run opens and follows the current file's AI log popup. `H` toggles that popup; navigating away does not discard that file's session output. `L` opens the global AI activity pane, which lists running and recent sessions across files and jumps back to the selected file/session with `Enter`. Press `O` or `o` in the AI activity pane or AI progress popup to open the AI log in an external pager.

Comments and AI logs are intentionally separate. Comments remain durable review state anchored to file lines and ranges. AI logs are transient session transcripts from ACP, Pi RPC, or CLI transport events, including provider startup/config failures. Agent output becomes a review reply only through the explicit AI reply/refactor flow that persists a reply on the target thread.

## AI agent transports

Parley prefers persistent agent transports over one-shot CLI prompt execution:

- `opencode`: ACP via `opencode acp`
- `codex`: ACP via `codex-acp`
- `claude`: ACP via `claude-agent-acp`
- `pi`: persistent JSONL RPC via `pi --mode rpc --no-session`, not ACP

Provider config still supports `transport = "cli"` as an explicit fallback. ACP agents stream `session/update` events into the per-file AI logs, and final thread replies are built from agent message chunks rather than thought chunks.

If an older config points ACP transport at a one-shot CLI command such as `codex exec`, `claude -p`, or `opencode run`, Parley rejects the run before spawning the process and shows the config error in the AI logs. Configure an ACP-capable command such as `codex-acp`, `claude-agent-acp`, or `opencode acp`, or set `transport = "cli"` when one-shot CLI mode is intentional.

Use `i` in the TUI to cycle the active AI provider. The active provider is shown in the status panel.

Use `I` in the TUI to toggle the active AI transport between ACP and CLI for providers that support both. The selected transport is saved as `ai.default_transport`, which accepts only the generic `acp` and `cli` choices. Pi ignores the toggle and keeps using provider-specific `pi_rpc`.

### `$HOME/.config/parley/config.toml` AI provider config

Parley reads user config from `$HOME/.config/parley/config.toml`. If the file is missing, these AI defaults are used:

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

Use CLI transport only for explicit one-shot command mode:

```toml
[ai.codex]
transport = "cli"
client = "codex"
args = ["exec"]

[ai.claude]
transport = "cli"
client = "claude"
args = ["-p"]

[ai.opencode]
transport = "cli"
client = "opencode"
args = ["run"]
```

Custom prompt templates can be configured globally or per AI mode:

```toml
[ai]
prompt_path = "prompts/ai.md"
reply_prompt_path = "prompts/reply.md"
refactor_prompt_path = "prompts/refactor.md"
```

## Local state and diff filtering

Parley stores config and review-owned data under the active store. By default, that store is global and repo-scoped:

```text
$HOME/.config/parley/repos/<repo-name>-<hash>/
  config.toml
  reviews/
    <review-name>/
      review.json
      logs/
        tui.log
```

If `.parley/` exists in the repository, Parley uses it as the active store:

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

Those `.parley/` files are ignored by default when Parley builds the review diff, so local review metadata does not pollute the file sidebar. This behavior is configurable through `$HOME/.config/parley/config.toml`:

```toml
ignore_parley_dir = false
```

## What `pending` means

- `pending` indicates the thread is waiting on counterpart follow-up after a reply.
- A thread returns to `open` when the original author replies again or explicitly marks it open.

## Completion behavior

- TUI completion is per-thread: `a` marks the selected thread addressed, and `o` reopens it.
