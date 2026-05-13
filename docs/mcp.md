# MCP Integration

Parley exposes an MCP-compatible JSON-RPC interface over stdio.

## Transport

- Supports both `Content-Length` framed and newline-delimited JSON-RPC messages.
- Implements `initialize`, `tools/list`, `tools/call`, `resources/list`, and `resources/read`.
- Advertises embedded documentation through MCP resources with `parley://docs/{slug}` URIs.

## Typical tools

- `list_reviews`
- `get_review`
- `list_open_comments`
- `add_reply`
- `mark_comment_addressed`
- `mark_comment_open`
- `set_review_state`
- `run_ai_session`
- `get_documentation`

## Embedded documentation

Agents can discover Parley's built-in markdown docs through `resources/list` and read any page with `resources/read`.

Available resource URIs:

- `parley://docs/keybindings`
- `parley://docs/overview`
- `parley://docs/quickstart`
- `parley://docs/review-workflow`
- `parley://docs/mcp`

Agents that prefer tools can call `get_documentation`.

Inputs:

- `doc`: optional doc slug, title, source path, or URI. If omitted, the tool returns the available docs list.

## `run_ai_session` behavior

Inputs:

- `provider`: `codex` | `claude` | `opencode` | `pi`
- `mode`: `reply` | `refactor` (optional; defaults by API call site)
- `comment_ids`: optional explicit thread IDs

Global gate:

- review-wide AI selects threads by thread status; explicit comment IDs target those comments directly

Target filtering:

- `mode=refactor`:
  - auto-target (`comment_ids` omitted): only `open` and `pending` threads
  - explicit `comment_ids`: still only `open` and `pending` threads are processed
- `mode=reply`:
  - auto-target (`comment_ids` omitted): `open` and `pending`
  - explicit `comment_ids`: status filter is bypassed for selection, so `addressed` can be processed

After processing:

- AI response is added as a thread reply
- thread status typically becomes `pending` (different-author reply path)

## Example calls

Reply mode over all eligible threads:

```json
{
  "name": "run_ai_session",
  "arguments": {
    "review_name": "my-review",
    "provider": "codex",
    "mode": "reply"
  }
}
```

Refactor mode over unresolved threads:

```json
{
  "name": "run_ai_session",
  "arguments": {
    "review_name": "my-review",
    "provider": "codex",
    "mode": "refactor"
  }
}
```

Reply mode on explicit thread IDs:

```json
{
  "name": "run_ai_session",
  "arguments": {
    "review_name": "my-review",
    "provider": "codex",
    "mode": "reply",
    "comment_ids": [12, 18]
  }
}
```

## Notes

- Tools that operate on review state require an explicit `review_name`.
- Thread state updates are explicit tool calls.
- `set_review_state` accepts Parley review states (`open`, `under_review`).
- `run_ai_session` supports `reply` and `refactor` modes.
- Supported AI providers: `codex`, `claude`, `opencode`, `pi`.
