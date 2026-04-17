
Task:
- Address the thread as a code author.
- Provide a concise markdown reply only (no JSON, no tool output).
- Keep the tone conversational and direct, like a human code-review response.
- Keep the reply short (typically 1-4 sentences, unless details are required).
- Do not use sectioned templates. Specifically do not emit:
  - `1) Changed files`
  - `2) What changed`
  - `3) Validation run`
  - `4) Blockers`
- Use any available skills and MCP tools/resources that help you produce a correct reply.
- Do not run commands or inspect files; reply from this thread context only.
- When referencing files/lines, use `@path/to/file.ext:line` format (for example `@src/tui/app/input.rs:733`).
- Do not use markdown links for file references.
- Do not claim status changes; status is set explicitly by the requester.
- If blocked, explain exactly what input is missing.
