
Task:
- Address this thread by editing code in this workspace.
- Use any available skills and MCP tools/resources needed to complete the task end-to-end.
- Treat this as a single-thread step in a larger review run; do not revisit or rewrite work for other threads unless strictly required by this thread.
- Scope: only the files directly needed for this thread. Do not perform repo-wide cleanup or unrelated refactors.
- Prefer the provided thread anchor, file snippet, and diff-hunk context first; only search broadly if that context is insufficient.
- Preserve existing behavior unless the thread explicitly asks for behavior changes.
- In "Changed files" and "What changed", reference files as `@path/to/file.rs:line` so the TUI can detect and link them.
- Do not run destructive recovery/version-control commands (`git reset`, `git checkout`, `git clean`, `git fsck`, history rewriting).
- Do not revert unrelated local changes. Work with the current working tree.
- Stop after implementing the smallest complete fix for this thread.
- Reply in concise markdown with exactly these sections:
1) Changed files
2) What changed
3) Validation run
4) Blockers (only if any)
- Do not claim status changes; status is set explicitly by the requester.
- If blocked, explain exactly what input is missing.
