use super::args::{AiProviderArg, AiSessionModeArg, AuthorArg, SideArg, StateArg};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "parley",
    about = "Local AI code review sessions for git changes"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Parser)]
pub enum Command {
    #[command(name = "tui")]
    Tui {
        /// Review name to open in the TUI.
        #[arg(long, required = true)]
        review: Option<String>,
        /// Disable mouse capture and mouse interaction in the TUI.
        #[arg(long)]
        no_mouse: bool,
        /// Show diff for a single commit (against its first parent).
        #[arg(long, conflicts_with_all = &["base", "head"])]
        commit: Option<String>,
        /// Review current repository files without requiring a git diff.
        #[arg(long, conflicts_with_all = &["commit", "base", "head"])]
        root: bool,
        /// Base revision for an explicit diff range.
        #[arg(long, conflicts_with = "commit")]
        base: Option<String>,
        /// Head revision for an explicit diff range (defaults to HEAD).
        #[arg(long, requires = "base", conflicts_with = "commit")]
        head: Option<String>,
    },
    #[command(name = "review")]
    Review {
        #[command(subcommand)]
        command: ReviewCommand,
    },
    #[command(name = "mcp")]
    Mcp,
}

#[derive(Debug, Parser)]
pub enum ReviewCommand {
    #[command(name = "create")]
    Create { name: String },
    #[command(name = "start")]
    Start { name: String },
    #[command(name = "list")]
    List,
    #[command(name = "show")]
    Show {
        name: String,
        /// Print review details as pretty JSON.
        #[arg(long)]
        json: bool,
    },
    #[command(name = "set-state")]
    SetState { name: String, state: StateArg },
    #[command(name = "add-comment")]
    AddComment {
        name: String,
        /// File path for the comment location.
        #[arg(long)]
        file: String,
        /// Diff side for the comment location (`left` or `right`).
        #[arg(long)]
        side: SideArg,
        /// Line number on the old (left) side of the diff.
        #[arg(long)]
        old_line: Option<u32>,
        /// Line number on the new (right) side of the diff.
        #[arg(long)]
        new_line: Option<u32>,
        /// Comment text body.
        #[arg(long)]
        body: String,
        /// Comment author (`user` or `ai`, default: `user`).
        #[arg(long, default_value = "user")]
        author: AuthorArg,
    },
    #[command(name = "add-reply")]
    AddReply {
        name: String,
        /// Target comment id to reply to.
        #[arg(long)]
        comment_id: u64,
        /// Reply text body.
        #[arg(long)]
        body: String,
        /// Reply author (`user` or `ai`, default: `ai`).
        #[arg(long, default_value = "ai")]
        author: AuthorArg,
    },
    #[command(name = "mark-addressed")]
    MarkAddressed {
        name: String,
        /// Target comment id to mark as addressed.
        #[arg(long)]
        comment_id: u64,
        /// Actor marking the comment (`user` or `ai`, default: `user`).
        #[arg(long, default_value = "user")]
        author: AuthorArg,
    },
    #[command(name = "mark-open")]
    MarkOpen {
        name: String,
        /// Target comment id to mark as open.
        #[arg(long)]
        comment_id: u64,
        /// Actor reopening the comment (`user` or `ai`, default: `user`).
        #[arg(long, default_value = "user")]
        author: AuthorArg,
    },
    #[command(name = "run-ai-session")]
    RunAiSession {
        name: String,
        /// AI provider to run for the session.
        #[arg(long)]
        provider: AiProviderArg,
        /// Session mode override (for example `reply` or `refactor`).
        #[arg(long)]
        mode: Option<AiSessionModeArg>,
        /// One or more comment ids to target (repeat `--comment-id`).
        #[arg(long = "comment-id")]
        comment_ids: Vec<u64>,
    },
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use clap::Parser;

    #[test]
    fn tui_command_parses_no_mouse_flag() {
        let cli = Cli::parse_from(["parley", "tui", "--review", "parser-cleanup", "--no-mouse"]);

        match cli.command {
            Command::Tui {
                review,
                no_mouse,
                commit,
                root,
                base,
                head,
            } => {
                assert_eq!(review.as_deref(), Some("parser-cleanup"));
                assert!(no_mouse);
                assert_eq!(commit, None);
                assert!(!root);
                assert_eq!(base, None);
                assert_eq!(head, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn tui_command_parses_commit_source() {
        let cli = Cli::parse_from([
            "parley",
            "tui",
            "--review",
            "parser-cleanup",
            "--commit",
            "HEAD~2",
        ]);

        match cli.command {
            Command::Tui {
                commit,
                root,
                base,
                head,
                ..
            } => {
                assert_eq!(commit.as_deref(), Some("HEAD~2"));
                assert!(!root);
                assert_eq!(base, None);
                assert_eq!(head, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn tui_command_requires_review_name() {
        let error = Cli::try_parse_from(["parley", "tui", "--commit", "HEAD~2"])
            .expect_err("cli should require review name");

        let message = error.to_string();
        assert!(message.contains("--review"));
    }

    #[test]
    fn tui_command_rejects_head_without_base() {
        let error = Cli::try_parse_from(["parley", "tui", "--head", "HEAD~1"])
            .expect_err("cli should reject head without base");

        let message = error.to_string();
        assert!(message.contains("--base"));
    }

    #[test]
    fn tui_command_rejects_commit_and_base_combination() {
        let error = Cli::try_parse_from(["parley", "tui", "--commit", "HEAD", "--base", "HEAD~1"])
            .expect_err("cli should reject conflicting diff sources");

        let message = error.to_string();
        assert!(message.contains("--commit"));
        assert!(message.contains("--base"));
    }

    #[test]
    fn tui_command_parses_root_source() {
        let cli = Cli::parse_from(["parley", "tui", "--review", "root-review", "--root"]);

        match cli.command {
            Command::Tui { review, root, .. } => {
                assert_eq!(review.as_deref(), Some("root-review"));
                assert!(root);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn tui_command_requires_review_name_with_root() {
        let error = Cli::try_parse_from(["parley", "tui", "--root"])
            .expect_err("cli should require review name with root");

        let message = error.to_string();
        assert!(message.contains("--review"));
    }

    #[test]
    fn tui_command_rejects_root_and_commit_combination() {
        let error = Cli::try_parse_from([
            "parley",
            "tui",
            "--review",
            "root-review",
            "--root",
            "--commit",
            "HEAD",
        ])
        .expect_err("cli should reject conflicting root and commit sources");

        let message = error.to_string();
        assert!(message.contains("--root"));
        assert!(message.contains("--commit"));
    }
}
