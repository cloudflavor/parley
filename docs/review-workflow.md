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
- Re-anchor: move thread to a different diff line -> `addressed` (status unchanged)

## 3. Review state reconciliation

Review state is reconciled from thread statuses:

- if any thread is `open`, review is `open`
- else if no thread is `open`, review is `under_review`
- TUI completion is handled per thread with `addressed`.

Review-level completion is not exposed by the TUI. Thread `addressed` is the completion signal.

## 4. Threading practice

Use line-level comments for actionable feedback. Keep one issue per thread so resolution is obvious.

## 5. Drive thread state deliberately

- Use `open` when code changes are required.
- Use `pending` when a reply is waiting on counterpart action.
- Use `addressed` when the original reviewer confirms resolution.

Example:

- reviewer leaves a comment -> `open`
- author replies "fixed in 9d2b3af" -> `pending`
- reviewer checks the change and is still unhappy -> reply reopens thread to `open`
- reviewer is satisfied -> mark `addressed`

## 6. AI eligibility matrix (what status to use before sending to AI)

### Mode = `refactor`

- No explicit `comment_ids` (auto-target):
  - processed: `open`, `pending`
  - skipped: `addressed`
- Explicit `comment_ids`:
  - processed: any selected status
  - skipped by status filter: none

What this means:

- review-wide AI refactor selects `open` and `pending` threads by default
- explicit selected-thread refactor targets the selected thread unless it is `addressed`

### Mode = `reply`

- No explicit `comment_ids` (auto-target):
  - processed: `open`, `pending`
  - skipped: `addressed`
- Explicit `comment_ids`:
  - processed: any selected status (including `addressed`)
  - skipped by status filter: none

What this means:

- for normal reply runs, use `open` or `pending`
- explicit selected-thread reply targets the selected thread unless it is `addressed`

## 7. Post-AI behavior

- AI output is persisted as a reply in the target thread.
- In typical human-authored threads, this sets thread status to `pending`.
- Review state then reconciles based on resulting thread statuses.

Example:

- thread is `open`
- you press `x` for AI refactor
- AI posts a reply into that thread
- thread becomes `pending`
- you inspect the code and either mark it `addressed` or reopen it

## 8. Refresh after code edits

After code edits or automation runs, refresh in TUI so thread anchors and diff context stay current.

If the TUI was opened with `--commit` or `--base` / `--head`, refresh keeps using that same revision source instead of falling back to the working tree.

## 9. Reviewing historical revisions

Parley can review more than the live working tree:

- `parley tui --commit <rev>`: diff that commit against its first parent
- `parley tui --base <rev>`: diff `<rev>..HEAD`
- `parley tui --base <rev> --head <rev>`: diff an explicit base/head range
- `parley tui --base <rev> --head HEAD`: diff everything after `<rev>`
- `parley tui --base <rev>^ --head HEAD`: diff everything after and including `<rev>`

AI sessions use the same selected revision source for prompt context.

## 10. Reviewing the current root directory

Use `parley tui --review <name> --root` to review the current repository root without requiring a git diff or commit. `--review` is required, and the review must already exist. Create it first with `parley review create <name>`.

Root-directory review mode loads tracked files plus untracked files that are not ignored by gitignore rules. It skips `.git/`, `.parley/`, and `worktrees/` directories. Each file is shown as context lines so comments can attach to the current file line numbers.

Root-directory review mode lazy-loads file content for startup performance. The TUI shows the file tree first, shows progress while file data loads, and opens file content when selected or when code search jumps to a match. JSON files are pretty-printed for display, and Markdown files are rendered into readable text rows. Press `D` / `Shift+d` to toggle between raw source and rendered document view.

## 11. Searching and hotspot review

Use `/` to search within the current file. Use `Ctrl+g` or command palette `Search Codebase` to search the repository from inside the TUI. Parley uses `rg` when available and falls back to `grep`; codebase results update while typing and `Enter` or mouse click opens the selected file and line.

Use `M` or command palette `Show Git File Heatmap` to scan git history on demand. The heatmap ranks files by the selected metric and colors each file cell from the active sort value.

Heatmap sort modes:

- `churn`: added plus removed lines
- `added`: total lines added
- `removed`: total lines removed
- `commits`: commits touching the file
- `net-growth`: added minus removed lines
- `net-shrink`: removed minus added lines
- `volatility`: churn per touching commit
- `path`: path name

Inside the heatmap, `s` cycles sort mode and `Shift+S` reverses direction.

## 12. Custom AI task prompts

Parley's AI prompt has two parts:

- generated review context from the selected thread, replies, target file, diff hunk, and referenced files
- task instructions for the selected AI mode

Configure markdown files in `$HOME/.config/parley/config.toml` to replace the task-instruction part:

```toml
[ai]
prompt_path = "prompts/parley-ai.md"
reply_prompt_path = "prompts/parley-reply.md"
refactor_prompt_path = "prompts/parley-refactor.md"
```

`reply_prompt_path` is used for reply mode, and `refactor_prompt_path` is used for refactor mode. If a mode-specific path is absent, Parley falls back to `prompt_path`. If no custom prompt path is configured, Parley uses the built-in prompt for that mode.

Relative paths are resolved from the repository root where Parley is started. If a configured markdown file cannot be read, the AI session fails before invoking the provider.

Inside the TUI, `Ctrl+k` opens the command palette. Select `Open Commit Picker` to choose from recent commits, filter by message or SHA, and press `Enter` to apply the selected commit as the active diff source.

Select `Open Review Picker` from the same command palette to switch review context. This reloads comments, replies, and review state from the selected review while keeping the active working tree, commit, or range diff unchanged.

Select `Create Review` to create a new review context from inside the TUI and switch to it immediately. New comments then persist under that review's directory.

Current limitation:

- the revision source is not stored in the review session yet, so a later reopen still needs the same CLI flags.
