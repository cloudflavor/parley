pub mod cli;
pub mod domain;
pub mod git;
pub mod mcp;
pub mod persistence;
pub mod services;
pub mod tui;

use anyhow::{Context, Result, anyhow};
use structopt::StructOpt;

use crate::{
    cli::{Cli, Command, ReviewCommand},
    domain::review::ReviewState,
    git::{diff::DiffSource, review_name::resolve_tui_review_name},
    persistence::store::Store,
    services::{
        ai_session::{RunAiSessionInput, default_ai_session_mode, run_ai_session},
        review_service::{AddCommentInput, AddReplyInput, ReviewService},
    },
};

pub async fn run() -> Result<()> {
    let cli = Cli::from_args();

    let project_root =
        std::env::current_dir().context("failed to read current working directory")?;
    let store = Store::from_project_root(&project_root);
    store.ensure_dirs().await?;
    let service = ReviewService::new(store);

    match cli.command {
        Command::Tui {
            review,
            no_mouse,
            commit,
            base,
            head,
        } => {
            let review = resolve_default_review_for_tui(&service, review.as_deref()).await?;
            let diff_source = resolve_tui_diff_source(commit, base, head);
            tui::run_tui(service, review, no_mouse, diff_source).await?;
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

async fn resolve_default_review_for_tui(
    service: &ReviewService,
    explicit: Option<&str>,
) -> Result<String> {
    let resolved = resolve_tui_review_name(explicit)?;
    if explicit.is_some() {
        return Ok(resolved);
    }

    if service.load_review(&resolved).await.is_ok() {
        return Ok(resolved);
    }

    let existing = service.list_reviews().await?;
    if existing.len() == 1 {
        return Ok(existing[0].clone());
    }

    Ok(resolved)
}

fn resolve_tui_diff_source(
    commit: Option<String>,
    base: Option<String>,
    head: Option<String>,
) -> DiffSource {
    if let Some(rev) = commit {
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
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "_".into()),
                        comment
                            .new_line
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "_".into()),
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
            let mode = mode
                .map(|value| value.0)
                .unwrap_or_else(|| default_ai_session_mode(&comment_ids));
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
