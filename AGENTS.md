# Agent Rules

- **Always use the `worktree` skill to create and delete worktrees.** Load it with `skill({ name: "worktree" })` at the start of every task. Never create or remove worktrees manually.
- Each body of work must begin in a new git worktree, not a branch in the main checkout.
- Run ALL commands from inside the worktree directory, never from the root checkout.
- When work is complete and merged, clean up the worktree using the `worktree` skill.
- Use `fff-mcp` to find files.
- Use the git-commit skill for creating commits. Always use -sS
- Use the Opal MCP to test changes and run tests in the pipeline.
- Use context7 to search for documentation and verify implementation
- **MOST IMPORTANT — At the end of EVERY change, BEFORE running the pipeline, ALWAYS run:** `cargo fmt && cargo check && cargo clippy --all-targets --all-features -- -D warnings`
  - **NEVER attempt to run the Opal pipeline without first verifying that `cargo fmt`, `cargo check`, and `cargo clippy` all pass.**
  - **If clippy/check fails, fix ALL errors before proceeding.**
  - **This is the highest priority rule. Do not skip this step.**
