# MCP Integration

Parley exposes an MCP-compatible JSON-RPC interface over stdio.

## Transport

- Uses `Content-Length` framed JSON-RPC messages.
- Implements `initialize`, `tools/list`, and `tools/call`.

## Typical tools

- `list_reviews`
- `get_review`
- `list_open_comments`
- `add_reply`
- `mark_comment_addressed`
- `mark_comment_open`
- `set_review_state`
- `run_ai_session`

## `run_ai_session` behavior

Inputs:

- `provider`: `codex` | `claude` | `opencode`
- `mode`: `reply` | `refactor` (optional; defaults by API call site)
- `comment_ids`: optional explicit thread IDs

Global gate:

- if review state is `done`, the AI session is skipped

Target filtering:

- `mode=refactor`:
  - auto-target (`comment_ids` omitted): only `open` threads
  - explicit `comment_ids`: still only `open` threads are processed
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

Refactor mode over open threads only:

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

- Review names can be resolved from current branch context.
- Thread state updates are explicit tool calls.
- `set_review_state` accepts Parley review states (`open`, `under_review`, `done`).
- `run_ai_session` supports `reply` and `refactor` modes.
