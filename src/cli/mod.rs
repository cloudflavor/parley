use std::str::FromStr;

use structopt::StructOpt;

use crate::domain::review::{Author, DiffSide, ReviewState};

#[derive(Debug, StructOpt)]
#[structopt(
    name = "parlar",
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
        #[structopt(long)]
        review: String,
        #[structopt(long)]
        theme: Option<String>,
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
    #[structopt(name = "list")]
    List,
    #[structopt(name = "show")]
    Show {
        name: String,
        #[structopt(long)]
        json: bool,
    },
    #[structopt(name = "set-state")]
    SetState { name: String, state: StateArg },
    #[structopt(name = "add-comment")]
    AddComment {
        name: String,
        #[structopt(long)]
        file: String,
        #[structopt(long)]
        side: SideArg,
        #[structopt(long)]
        old_line: Option<u32>,
        #[structopt(long)]
        new_line: Option<u32>,
        #[structopt(long)]
        body: String,
        #[structopt(long, default_value = "user")]
        author: AuthorArg,
    },
    #[structopt(name = "add-reply")]
    AddReply {
        name: String,
        #[structopt(long)]
        comment_id: u64,
        #[structopt(long)]
        body: String,
        #[structopt(long, default_value = "ai")]
        author: AuthorArg,
    },
    #[structopt(name = "mark-addressed")]
    MarkAddressed {
        name: String,
        #[structopt(long)]
        comment_id: u64,
        #[structopt(long, default_value = "ai")]
        author: AuthorArg,
    },
    #[structopt(name = "mark-open")]
    MarkOpen {
        name: String,
        #[structopt(long)]
        comment_id: u64,
        #[structopt(long, default_value = "user")]
        author: AuthorArg,
    },
    #[structopt(name = "done")]
    Done { name: String },
}

#[derive(Debug, Clone)]
pub struct StateArg(pub ReviewState);

impl FromStr for StateArg {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "draft" => Ok(Self(ReviewState::Draft)),
            "pending" => Ok(Self(ReviewState::Pending)),
            "waiting_for_response" => Ok(Self(ReviewState::WaitingForResponse)),
            "done" => Ok(Self(ReviewState::Done)),
            _ => Err(format!("invalid state: {input}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SideArg(pub DiffSide);

impl FromStr for SideArg {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "left" => Ok(Self(DiffSide::Left)),
            "right" => Ok(Self(DiffSide::Right)),
            _ => Err(format!("invalid side: {input}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthorArg(pub Author);

impl FromStr for AuthorArg {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "user" => Ok(Self(Author::User)),
            "ai" => Ok(Self(Author::Ai)),
            _ => Err(format!("invalid author: {input}")),
        }
    }
}
