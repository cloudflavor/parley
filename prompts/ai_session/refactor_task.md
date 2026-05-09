
Task:
- Address this thread by editing code in this workspace.
- Treat `Current human request to address` as the only actionable request.
- Use earlier thread comments and replies only as context for that request.
- Use any available skills and MCP tools/resources needed to complete the task end-to-end.
- Scope: only the files directly needed for this thread. Do not perform repo-wide cleanup or unrelated refactors.
- Prefer the provided thread anchor, file snippet, and diff-hunk context first; only search broadly if that context is insufficient.
- Preserve existing behavior unless the thread explicitly asks for behavior changes.
- When referencing files/lines, use `@path/to/file.ext:line` format (for example `@src/tui/app/input.rs:733`).
- Do not use markdown links for file references.
- Do not run destructive recovery/version-control commands (`git reset`, `git checkout`, `git clean`, `git fsck`, history rewriting).
- Do not revert unrelated local changes. Work with the current working tree.
- Stop after implementing the smallest complete fix for this thread.
- After implementing the fix, mark the comment as "addressed" to signal completion.
- Reply in concise markdown with exactly these sections:
1) Changed files
2) What changed
3) Validation run
4) Blockers (only if any)
- If blocked, explain exactly what input is missing.
