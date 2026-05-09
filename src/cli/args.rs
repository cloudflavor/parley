use std::str::FromStr;

use thiserror::Error;

use crate::domain::{
    ai::{AiProvider, AiSessionMode},
    review::{Author, DiffSide, ReviewState},
};

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
        match input {
            "open" => Ok(Self(ReviewState::Open)),
            "under_review" => Ok(Self(ReviewState::UnderReview)),
            "done" => Ok(Self(ReviewState::Done)),
            _ => Err(CliArgError::InvalidState {
                value: input.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SideArg(pub DiffSide);

impl FromStr for SideArg {
    type Err = CliArgError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "left" => Ok(Self(DiffSide::Left)),
            "right" => Ok(Self(DiffSide::Right)),
            _ => Err(CliArgError::InvalidSide {
                value: input.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthorArg(pub Author);

impl FromStr for AuthorArg {
    type Err = CliArgError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "user" => Ok(Self(Author::User)),
            "ai" => Ok(Self(Author::Ai)),
            _ => Err(CliArgError::InvalidAuthor {
                value: input.to_string(),
            }),
        }
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
