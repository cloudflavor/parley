# Agent Rules

- Each body of work must begin in a new git worktree created with `git worktree`, not a branch in the main checkout.
- Create each worktree inside the repository root under `worktrees/<body-of-work>`.
- Run ALL commands from inside the worktree directory, never from the root checkout.
- When work is complete and merged, remove only the specific `worktrees/<body-of-work>` directory created for that body of work.
- Do not delete, prune, or modify any other directories under `worktrees/`.
- Use `fff-mcp` to find files.
- Use the git-commit skill for creating commits. Always use -sS
- Use context7 to search for documentation and verify implementation
- **MOST IMPORTANT — At the end of EVERY change, BEFORE informing the user to run the pipeline, ALWAYS run:**
  1. `cargo fmt && cargo check && cargo clippy --all-targets --all-features -- -D warnings`
  2. `cargo test --all-targets --all-features`
  - **NEVER tell the user to run the Opal pipeline without first verifying that ALL checks AND ALL tests pass.**
  - **If clippy/check/tests fail, fix ALL errors before proceeding.**
  - **This is the highest priority rule. Do not skip this step.**
