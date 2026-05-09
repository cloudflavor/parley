# Agent Rules

- **Always use the `worktree` skill to create and delete worktrees.** Load it with `skill({ name: "worktree" })` at the start of every task. Never create or remove worktrees manually.
- Each body of work must begin in a new git worktree, not a branch in the main checkout.
- Run ALL commands from inside the worktree directory, never from the root checkout.
- When work is complete and merged, clean up the worktree using the `worktree` skill.
- Use `fff-mcp` to find files.
- Use the git-commit skill for creating commits. Always use -sS
- Use context7 to search for documentation and verify implementation
- **MOST IMPORTANT — At the end of EVERY change, BEFORE informing the user to run the pipeline, ALWAYS run:**
  1. `cargo fmt && cargo check && cargo clippy --all-targets --all-features -- -D warnings`
  2. `cargo test --all-targets --all-features`
  - **NEVER tell the user to run the Opal pipeline without first verifying that ALL checks AND ALL tests pass.**
  - **If clippy/check/tests fail, fix ALL errors before proceeding.**
  - **This is the highest priority rule. Do not skip this step.**
