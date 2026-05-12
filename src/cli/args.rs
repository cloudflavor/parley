use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::review::{Author, DiffSide, ReviewState};
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum CliArgError {
    #[error("invalid state: {value}")]
    InvalidState { value: String },
    #[error("invalid side: {value}")]
    InvalidSide { value: String },
    #[error("invalid author: {value}")]
    InvalidAuthor { value: String },
    #[error("invalid ai provider: {value}")]
    InvalidAiProvider { value: String },
    #[error("invalid ai session mode: {value}")]
    InvalidAiSessionMode { value: String },
}

#[derive(Debug, Clone)]
pub struct StateArg(pub ReviewState);

impl FromStr for StateArg {
    type Err = CliArgError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<ReviewState>()
            .map(Self)
            .map_err(|_| CliArgError::InvalidState {
                value: input.to_string(),
            })
    }
}

#[derive(Debug, Clone)]
pub struct SideArg(pub DiffSide);

impl FromStr for SideArg {
    type Err = CliArgError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<DiffSide>()
            .map(Self)
            .map_err(|_| CliArgError::InvalidSide {
                value: input.to_string(),
            })
    }
}

#[derive(Debug, Clone)]
pub struct AuthorArg(pub Author);

impl FromStr for AuthorArg {
    type Err = CliArgError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<Author>()
            .map(Self)
            .map_err(|_| CliArgError::InvalidAuthor {
                value: input.to_string(),
            })
    }
}

#[derive(Debug, Clone)]
pub struct AiProviderArg(pub AiProvider);

impl FromStr for AiProviderArg {
    type Err = CliArgError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<AiProvider>()
            .map(Self)
            .map_err(|_| CliArgError::InvalidAiProvider {
                value: input.to_string(),
            })
    }
}

#[derive(Debug, Clone)]
pub struct AiSessionModeArg(pub AiSessionMode);

impl FromStr for AiSessionModeArg {
    type Err = CliArgError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<AiSessionMode>()
            .map(Self)
            .map_err(|_| CliArgError::InvalidAiSessionMode {
                value: input.to_string(),
            })
    }
}
