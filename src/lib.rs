use crate::cli::{Cli, Command, ConfigCommand, ReviewCommand, WorktreeCommand};
use crate::domain::review::ReviewState;
use crate::git::diff::DiffSource;
use crate::git::worktree::{
    RepositoryContext, discover_from_cwd, discover_with_worktree, list_worktrees,
};
use crate::persistence::store::Store;
use crate::services::ai_session::{RunAiSessionInput, default_ai_session_mode, run_ai_session};
use crate::services::review_service::{AddCommentInput, AddReplyInput, ReviewService};
use anyhow::{Context, Result, anyhow};
use clap::Parser;
use std::env;
use std::ffi::OsString;
use std::io::{IsTerminal, stdin, stdout};
use tokio::fs;

pub mod cli;
pub mod docs;
pub mod domain;
pub mod error;
pub mod git;
pub mod mcp;
pub mod persistence;
pub mod services;
pub mod tui;
pub mod utils;

/// # Errors
///
/// Returns an error when CLI command handling, repository access, persistence, MCP I/O, or TUI
/// execution fails.
pub async fn run() -> Result<()> {
    let args: Vec<OsString> = env::args_os().collect();
    let cli = if should_run_mcp(&args) {
        Cli {
            worktree: None,
            command: Command::Mcp,
        }
    } else {
        Cli::parse()
    };

    let ctx = if let Some(ref wt) = cli.worktree {
        discover_with_worktree(
            env::current_dir().context("failed to read cwd")?,
            Some(wt.as_str()),
        )
        .await?
    } else {
        discover_from_cwd().await?
    };

    match cli.command {
        Command::Config { command } => {
            handle_config_command(command, &ctx).await?;
        }
        Command::Tui {
            review,
            no_mouse,
            commit,
            root,
            base,
            head,
        } => {
            let store = Store::resolve_from_context(&ctx).await?;
            let service = ReviewService::new(store);
            let diff_source = resolve_tui_diff_source(commit, root, base, head);
            let review_name = review.context("missing required --review")?;
            tui::run_tui(service, review_name, no_mouse, diff_source, false, &ctx).await?;
        }
        Command::Review { command } => {
            let store = Store::resolve_from_context(&ctx).await?;
            let service = ReviewService::new(store);
            handle_review_command(command, &service, &ctx).await?;
        }
        Command::Mcp => {
            let store = Store::resolve_from_context(&ctx).await?;
            let service = ReviewService::new(store);
            mcp::run_mcp(service).await?;
        }
        Command::Worktree { command } => {
            handle_worktree_command(command, &ctx).await?;
        }
    }

    Ok(())
}

fn should_run_mcp(args: &[OsString]) -> bool {
    if args.len() == 1 && !stdin().is_terminal() && !stdout().is_terminal() {
        return true;
    }

    let first_arg = args.get(1).and_then(|value| value.to_str());
    if matches!(first_arg, Some("mcp")) {
        return true;
    }

    args.iter()
        .skip(1)
        .filter_map(|value| value.to_str())
        .any(|value| matches!(value, "--stdio" | "--mcp"))
}

fn resolve_tui_diff_source(
    commit: Option<String>,
    root: bool,
    base: Option<String>,
    head: Option<String>,
) -> DiffSource {
    if root {
        DiffSource::RootDirectory
    } else if let Some(rev) = commit {
        DiffSource::Commit { rev }
    } else if let Some(base) = base {
        DiffSource::Range {
            base,
            head: head.unwrap_or_else(|| "HEAD".to_string()),
        }
    } else {
        DiffSource::WorkingTree
    }
}

async fn handle_config_command(command: ConfigCommand, ctx: &RepositoryContext) -> Result<()> {
    match command {
        ConfigCommand::Path => {
            println!("{}", ctx.storage_root.display());
        }
        ConfigCommand::UseLocal => {
            let local_root = ctx.selected_worktree.join(".parley");
            fs::create_dir_all(&local_root)
                .await
                .with_context(|| format!("failed to create {}", local_root.display()))?;
            let store = Store::from_project_root(&ctx.selected_worktree);
            store.ensure_dirs().await?;
            println!("{}", store.root_path().display());
        }
    }

    Ok(())
}

async fn handle_worktree_command(command: WorktreeCommand, ctx: &RepositoryContext) -> Result<()> {
    match command {
        WorktreeCommand::List => {
            let worktrees = list_worktrees(&ctx.selected_worktree).await?;
            for wt in worktrees {
                let marker = if wt.is_current { " *" } else { "" };
                let branch = wt
                    .branch
                    .as_deref()
                    .or(wt.head_summary.as_deref())
                    .unwrap_or("-");
                println!("{}{}\t{}\t{}", wt.name, marker, wt.path.display(), branch);
            }
        }
        WorktreeCommand::Current => {
            println!("{}", ctx.selected_worktree.display());
            if let Some(ref name) = ctx.current_worktree_name {
                println!("{name}");
            }
        }
    }
    Ok(())
}

async fn handle_review_command(
    command: ReviewCommand,
    service: &ReviewService,
    ctx: &RepositoryContext,
) -> Result<()> {
    match command {
        ReviewCommand::Create { name } => {
            let review = service.create_review(&name).await?;
            println!("created review {} in {:?}", review.name, review.state);
        }
        ReviewCommand::Start { name } => {
            let review = service.set_state(&name, ReviewState::UnderReview).await?;
            println!("review {} started in {:?}", review.name, review.state);
        }
        ReviewCommand::List => {
            for review_name in service.list_reviews().await? {
                println!("{review_name}");
            }
        }
        ReviewCommand::Show { name, json } => {
            let review = service.load_review(&name).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&review)?);
            } else {
                println!("name: {}", review.name);
                println!("state: {:?}", review.state);
                println!("comments: {}", review.comments.len());
                for comment in review.comments {
                    println!(
                        "  #{} [{}] {}:{} {}",
                        comment.id,
                        comment.status.as_str(),
                        comment
                            .old_line
                            .map_or_else(|| "_".into(), |value| value.to_string()),
                        comment
                            .new_line
                            .map_or_else(|| "_".into(), |value| value.to_string()),
                        comment.body
                    );
                }
            }
        }
        ReviewCommand::SetState { name, state } => {
            let review = service.set_state(&name, state.0).await?;
            println!("state updated to {:?}", review.state);
        }
        ReviewCommand::AddComment {
            name,
            file,
            side,
            old_line,
            new_line,
            body,
            author,
        } => {
            if old_line.is_none() && new_line.is_none() {
                return Err(anyhow!("provide --old-line or --new-line"));
            }

            let review = service
                .add_comment(
                    &name,
                    AddCommentInput {
                        file_path: file,
                        old_line,
                        new_line,
                        line_range: None,
                        side: side.0,
                        line_anchor: None,
                        original_anchor: None,
                        body,
                        author: author.0,
                    },
                )
                .await?;
            println!("comment added. total comments: {}", review.comments.len());
        }
        ReviewCommand::AddReply {
            name,
            comment_id,
            body,
            author,
        } => {
            service
                .add_reply(
                    &name,
                    AddReplyInput {
                        comment_id,
                        author: author.0,
                        body,
                    },
                )
                .await?;
            println!("reply added to comment #{comment_id}");
        }
        ReviewCommand::MarkAddressed {
            name,
            comment_id,
            author,
        } => {
            service.mark_addressed(&name, comment_id, author.0).await?;
            println!("comment #{comment_id} marked addressed");
        }
        ReviewCommand::MarkOpen {
            name,
            comment_id,
            author,
        } => {
            service.mark_open(&name, comment_id, author.0).await?;
            println!("comment #{comment_id} marked open");
        }
        ReviewCommand::RunAiSession {
            name,
            provider,
            mode,
            comment_ids,
        } => {
            let mode = mode.map_or_else(|| default_ai_session_mode(&comment_ids), |value| value.0);
            let result = run_ai_session(
                service,
                RunAiSessionInput {
                    review_name: name,
                    provider: provider.0,
                    transport: None,
                    comment_ids,
                    mode,
                    diff_source: DiffSource::WorkingTree,
                    worktree_path: Some(ctx.selected_worktree.clone()),
                },
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::handle_config_command;
    use super::should_run_mcp;
    use crate::cli::ConfigCommand;
    use crate::git::worktree::RepositoryContext;
    use std::ffi::OsString;
    use tempfile::tempdir;
    use tokio::fs as tokio_fs;

    #[test]
    fn should_run_mcp_when_first_arg_is_mcp() {
        let args = vec![OsString::from("parley"), OsString::from("mcp")];
        assert!(should_run_mcp(&args));
    }

    #[test]
    fn should_run_mcp_when_stdio_flag_is_present() {
        let args = vec![OsString::from("parley"), OsString::from("--stdio")];
        assert!(should_run_mcp(&args));
    }

    #[tokio::test]
    async fn config_use_local_should_create_local_store() -> anyhow::Result<()> {
        let tempdir = tempdir()?;
        let ctx = RepositoryContext {
            selected_worktree: tempdir.path().to_path_buf(),
            main_worktree: Some(tempdir.path().to_path_buf()),
            common_git_dir: tempdir.path().join(".git"),
            storage_root: tempdir.path().join(".parley"),
            current_worktree_name: None,
        };

        handle_config_command(ConfigCommand::UseLocal, &ctx).await?;

        assert!(tokio_fs::try_exists(tempdir.path().join(".parley/reviews")).await?);
        Ok(())
    }
}
