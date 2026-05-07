use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("git operation failed: {0}")]
    Git(#[from] git2::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("review not found: {0}")]
    ReviewNotFound(String),

    #[error("comment not found: {id}")]
    CommentNotFound { id: u64 },

    #[error("invalid diff side: {0}")]
    InvalidDiffSide(String),

    #[error("invalid review state: {0}")]
    InvalidReviewState(String),

    #[error("invalid author: {0}")]
    InvalidAuthor(String),

    #[error("invalid ai provider: {0}")]
    InvalidAiProvider(String),

    #[error("invalid ai session mode: {0}")]
    InvalidAiSessionMode(String),

    #[error("persistence error: {0}")]
    Persistence(String),

    #[error("tui error: {0}")]
    Tui(String),

    #[error("mcp error: {0}")]
    Mcp(String),

    #[error("ai service error: {0}")]
    AiService(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;
