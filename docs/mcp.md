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

## Notes

- Review names can be resolved from current branch context.
- Thread state updates remain explicit operations.
- AI runs operate within the same review state model and return structured outcomes.
