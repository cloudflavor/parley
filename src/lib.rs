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

use std::io::IsTerminal;
use std::{ffi::OsString, path::Path};

use anyhow::{Context, Result, anyhow};
use clap::Parser;

use crate::cli::{Cli, Command, ReviewCommand};
use crate::git::{diff::DiffSource, review_name::normalize_review_name};
use crate::services::review_service::ReviewService;

/// # Errors
///
/// Returns an error when CLI command handling, repository access, persistence, MCP I/O, or TUI
/// execution fails.
pub async fn run() -> Result<()> {
    let args: Vec<OsString> = std::env::args_os().collect();
    let command = if should_run_mcp(&args) {
        Command::Mcp
    } else {
        Cli::parse().command
    };

    let project_root =
        std::env::current_dir().context("failed to read current working directory")?;
    let store = crate::persistence::store::Store::from_project_root(&project_root);
    let service = ReviewService::new(store);

    match command {
        Command::Tui {
            review,
            no_mouse,
            commit,
            root,
            base,
            head,
        } => {
            let diff_source = resolve_tui_diff_source(commit, root, base, head);
            let review_name = review.unwrap_or_else(|| default_root_review_name(&project_root));
            tui::run_tui(service, review_name, no_mouse, diff_source, root).await?;
        }
        Command::Review { command } => {
            handle_review_command(command, &service).await?;
        }
        Command::Mcp => {
            mcp::run_mcp(service).await?;
        }
    }

    Ok(())
}

fn default_root_review_name(project_root: &Path) -> String {
    let raw_name = project_root
        .file_name()
        .and_then(|value| value.to_str())
        .map_or_else(|| "root".to_string(), |value| format!("{value}-root"));
    let normalized = normalize_review_name(&raw_name);
    if normalized.is_empty() {
        "root".to_string()
    } else {
        normalized
    }
}

fn should_run_mcp(args: &[OsString]) -> bool {
    if args.len() == 1 && !std::io::stdin().is_terminal() && !std::io::stdout().is_terminal() {
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

async fn handle_review_command(command: ReviewCommand, service: &ReviewService) -> Result<()> {
    use crate::domain::review::ReviewState;
    use crate::services::ai_session::{RunAiSessionInput, default_ai_session_mode, run_ai_session};
    use crate::services::review_service::{AddCommentInput, AddReplyInput};

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
                        match comment.status {
                            crate::domain::review::CommentStatus::Open => "open",
                            crate::domain::review::CommentStatus::Pending => "pending_human",
                            crate::domain::review::CommentStatus::Addressed => "addressed",
                        },
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
                        side: side.0,
                        line_anchor: None,
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
        ReviewCommand::Done { name } => {
            service.set_state(&name, ReviewState::Done).await?;
            println!("review {name} marked done");
        }
        ReviewCommand::Resolve { name } => {
            service.set_state(&name, ReviewState::Done).await?;
            println!("review {name} resolved");
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
                    comment_ids,
                    mode,
                    diff_source: DiffSource::WorkingTree,
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
    use super::should_run_mcp;
    use std::ffi::OsString;

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
}
