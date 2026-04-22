use structopt::StructOpt;

use super::args::{AiProviderArg, AiSessionModeArg, AuthorArg, SideArg, StateArg};

#[derive(Debug, StructOpt)]
#[structopt(
    name = "parley",
    about = "Local AI code review sessions for git changes"
)]
pub struct Cli {
    #[structopt(subcommand)]
    pub command: Command,
}

#[derive(Debug, StructOpt)]
pub enum Command {
    #[structopt(name = "tui")]
    Tui {
        /// Review name to open in the TUI.
        #[structopt(long)]
        review: Option<String>,
        /// Disable mouse capture and mouse interaction in the TUI.
        #[structopt(long)]
        no_mouse: bool,
        /// Show diff for a single commit (against its first parent).
        #[structopt(long, conflicts_with_all = &["base", "head"])]
        commit: Option<String>,
        /// Base revision for an explicit diff range.
        #[structopt(long, conflicts_with = "commit")]
        base: Option<String>,
        /// Head revision for an explicit diff range (defaults to HEAD).
        #[structopt(long, requires = "base", conflicts_with = "commit")]
        head: Option<String>,
    },
    #[structopt(name = "review")]
    Review {
        #[structopt(subcommand)]
        command: ReviewCommand,
    },
    #[structopt(name = "mcp")]
    Mcp,
}

#[derive(Debug, StructOpt)]
pub enum ReviewCommand {
    #[structopt(name = "create")]
    Create { name: String },
    #[structopt(name = "start")]
    Start { name: String },
    #[structopt(name = "list")]
    List,
    #[structopt(name = "show")]
    Show {
        name: String,
        /// Print review details as pretty JSON.
        #[structopt(long)]
        json: bool,
    },
    #[structopt(name = "set-state")]
    SetState { name: String, state: StateArg },
    #[structopt(name = "add-comment")]
    AddComment {
        name: String,
        /// File path for the comment location.
        #[structopt(long)]
        file: String,
        /// Diff side for the comment location (`left` or `right`).
        #[structopt(long)]
        side: SideArg,
        /// Line number on the old (left) side of the diff.
        #[structopt(long)]
        old_line: Option<u32>,
        /// Line number on the new (right) side of the diff.
        #[structopt(long)]
        new_line: Option<u32>,
        /// Comment text body.
        #[structopt(long)]
        body: String,
        /// Comment author (`user` or `ai`, default: `user`).
        #[structopt(long, default_value = "user")]
        author: AuthorArg,
    },
    #[structopt(name = "add-reply")]
    AddReply {
        name: String,
        /// Target comment id to reply to.
        #[structopt(long)]
        comment_id: u64,
        /// Reply text body.
        #[structopt(long)]
        body: String,
        /// Reply author (`user` or `ai`, default: `ai`).
        #[structopt(long, default_value = "ai")]
        author: AuthorArg,
    },
    #[structopt(name = "mark-addressed")]
    MarkAddressed {
        name: String,
        /// Target comment id to mark as addressed.
        #[structopt(long)]
        comment_id: u64,
        /// Actor marking the comment (`user` or `ai`, default: `user`).
        #[structopt(long, default_value = "user")]
        author: AuthorArg,
    },
    #[structopt(name = "mark-open")]
    MarkOpen {
        name: String,
        /// Target comment id to mark as open.
        #[structopt(long)]
        comment_id: u64,
        /// Actor reopening the comment (`user` or `ai`, default: `user`).
        #[structopt(long, default_value = "user")]
        author: AuthorArg,
    },
    #[structopt(name = "run-ai-session")]
    RunAiSession {
        name: String,
        /// AI provider to run for the session.
        #[structopt(long)]
        provider: AiProviderArg,
        /// Session mode override (for example `reply` or `refactor`).
        #[structopt(long)]
        mode: Option<AiSessionModeArg>,
        /// One or more comment ids to target (repeat `--comment-id`).
        #[structopt(long = "comment-id")]
        comment_ids: Vec<u64>,
    },
    #[structopt(name = "done")]
    Done { name: String },
    #[structopt(name = "resolve")]
    Resolve { name: String },
}

#[cfg(test)]
mod tests {
    use structopt::StructOpt;

    use super::{Cli, Command};

    #[test]
    fn tui_command_parses_no_mouse_flag() {
        let cli = Cli::from_iter_safe(["parley", "tui", "--review", "main", "--no-mouse"])
            .expect("cli should parse");

        match cli.command {
            Command::Tui {
                review,
                no_mouse,
                commit,
                base,
                head,
            } => {
                assert_eq!(review.as_deref(), Some("main"));
                assert!(no_mouse);
                assert_eq!(commit, None);
                assert_eq!(base, None);
                assert_eq!(head, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn tui_command_parses_commit_source() {
        let cli =
            Cli::from_iter_safe(["parley", "tui", "--commit", "HEAD~2"]).expect("cli should parse");

        match cli.command {
            Command::Tui {
                commit, base, head, ..
            } => {
                assert_eq!(commit.as_deref(), Some("HEAD~2"));
                assert_eq!(base, None);
                assert_eq!(head, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn tui_command_rejects_head_without_base() {
        let error = Cli::from_iter_safe(["parley", "tui", "--head", "HEAD~1"])
            .expect_err("cli should reject head without base");

        let message = error.to_string();
        assert!(message.contains("--base"));
    }

    #[test]
    fn tui_command_rejects_commit_and_base_combination() {
        let error = Cli::from_iter_safe(["parley", "tui", "--commit", "HEAD", "--base", "HEAD~1"])
            .expect_err("cli should reject conflicting diff sources");

        let message = error.to_string();
        assert!(message.contains("--commit"));
        assert!(message.contains("--base"));
    }
}
