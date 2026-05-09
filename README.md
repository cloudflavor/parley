# Parley

Parley is a terminal-first review tool for local Git diffs with structured, line-anchored discussion threads and optional AI-assisted replies/refactors.

It is designed for iterative review loops where code changes are generated or assisted by AI, but review state and thread resolution remain explicit and human-controlled.

Primary docs site: [https://parley.cloudflavor.io](https://parley.cloudflavor.io)

## Why Parley

- Review threads are anchored to specific diff lines, not loose notes.
- Thread status is explicit: `open`, `pending`, `addressed`.
- Review state is explicit: `open`, `under_review`, `done`.
- TUI workflow is keyboard-first and optimized for rapid navigation.
- AI operations integrate into the same thread model instead of bypassing review state.

## Core Concepts

### Diff source vs review session

Parley separates:

- Diff source: what code you are reviewing (`working tree`, `--commit`, or `--base/--head` range)
- Review session: local state under `.parley/` (review name, threaded comments, status history)

### Thread lifecycle

- New comment -> `open`
- Reply by original thread author -> `open`
- Reply by different author (including AI in normal flows) -> `pending`
- Explicit resolution by original thread author -> `addressed`

### Review lifecycle

- Any `open` thread -> review is `open`
- No `open` threads -> review is `under_review`
- `done` is explicit and guarded
- Normal `done` transition is blocked while unresolved threads exist (`open` or `pending`)

## Installation and Build

Prerequisites:

- Rust toolchain
- Git repository as working directory
- Terminal with TUI support

Build locally:

```bash
cargo build --release
```

Run from source:

```bash
cargo run -- tui
```

Install the `parley` binary locally:

```bash
cargo install --path .
```

## Quickstart

Create and start a review session:

```bash
parley review create my-review
parley review start my-review
```

Open TUI on current working tree changes:

```bash
parley tui --review my-review
```

Disable mouse capture for SSH/terminal compatibility:

```bash
parley tui --review my-review --no-mouse
```

Review historical diffs:

```bash
parley tui --review my-review --commit HEAD~2
parley tui --review my-review --base main --head feature/my-branch
parley tui --review my-review --base v0.1.0
# everything after HEAD~2 (exclude that commit)
parley tui --review my-review --base HEAD~2 --head HEAD
# everything after and including HEAD~2
parley tui --review my-review --base HEAD~2^ --head HEAD
```

## CLI Reference

Top-level commands:

- `parley tui`
- `parley search <query> [paths...]`
- `parley review <subcommand>`
- `parley mcp`

Search examples:

```bash
parley search "TODO"
parley search "TODO" src docs
```

Search uses `rg` when available. If `rg` is unavailable, it falls back to `grep` and honors `.gitignore` through Git file tracking for Git worktrees.

Common `review` subcommands:

- `create <name>`
- `start <name>`
- `list`
- `show <name> [--json]`
- `set-state <name> <open|under_review|done>`
- `add-comment ...`
- `add-reply ...`
- `mark-addressed ...`
- `mark-open ...`
- `run-ai-session ...`
- `done <name>`
- `resolve <name>`

## TUI Workflow and Key Controls

Thread actions:

- `m` or `c`: create thread on selected line
- `r`: reply to selected thread
- `a`: mark addressed
- `o`: mark open
- `f`: force-address selected thread
- `N` / `P`: next/previous thread

Review state actions:

- `s`: set `open`
- `w`: set `under_review`
- `d`: set `done` (guarded)
- `Shift+D`: force set `done`

AI actions:

- `x`: AI refactor selected thread
- `X`: AI reply selected thread
- `A`: AI refactor review
- `K`: cancel active AI run

Useful navigation:

- `h/l`: previous/next file
- `j/k`: line up/down
- `/query`: search
- `R`: refresh diff and review data
- `?`: in-app help

## AI Session Behavior

Providers:

- `codex`
- `claude`
- `opencode`

Modes:

- `refactor`
- `reply`

Eligibility summary:

- If review state is `done`, AI session is skipped.
- `refactor` targets `open` threads.
- `reply` targets `open` and `pending` by default.
- Explicit `comment_ids` can override default reply-mode filtering behavior.

## MCP Integration

Run MCP server over stdio:

```bash
parley mcp
```

Parley exposes JSON-RPC MCP tooling for review automation, including:

- `list_reviews`
- `get_review`
- `list_open_comments`
- `add_reply`
- `mark_comment_addressed`
- `mark_comment_open`
- `set_review_state`
- `run_ai_session`

## Local State and Configuration

Parley stores local review data under:

```text
.parley/
```

By default, `.parley/` is excluded from review diff file lists.  
To include it again, set:

```toml
ignore_parley_dir = false
```

in:

```text
.parley/config.toml
```

## Documentation

Main docs website:

- [parley.cloudflavor.io](https://parley.cloudflavor.io)

Project docs in this repository:

- [Overview](docs/overview.md)
- [Quickstart](docs/quickstart.md)
- [Review Workflow](docs/review-workflow.md)
- [Keybindings](docs/keybindings.md)
- [MCP Integration](docs/mcp.md)

Docs site source and deployment tooling:

- [ui-docs/README.md](ui-docs/README.md)

## License

Apache-2.0
