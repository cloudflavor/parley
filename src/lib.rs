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
    persistence::store::Store,
    services::review_service::{AddCommentInput, AddReplyInput, ReviewService},
};

pub async fn run() -> Result<()> {
    let cli = Cli::from_args();

    let project_root =
        std::env::current_dir().context("failed to read current working directory")?;
    let store = Store::from_project_root(&project_root);
    store.ensure_dirs().await?;
    let service = ReviewService::new(store);

    match cli.command {
        Command::Tui { review, theme } => {
            tui::run_tui(service, review, theme).await?;
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

async fn handle_review_command(command: ReviewCommand, service: &ReviewService) -> Result<()> {
    match command {
        ReviewCommand::Create { name } => {
            let review = service.create_review(&name).await?;
            println!("created review {} in {:?}", review.name, review.state);
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
    }

    Ok(())
}
