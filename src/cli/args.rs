use std::str::FromStr;

use crate::domain::{
    ai::{AiProvider, AiSessionMode},
    review::{Author, DiffSide, ReviewState},
};

#[derive(Debug, Clone)]
pub struct StateArg(pub ReviewState);

impl FromStr for StateArg {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "open" => Ok(Self(ReviewState::Open)),
            "under_review" => Ok(Self(ReviewState::UnderReview)),
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

#[derive(Debug, Clone)]
pub struct AiProviderArg(pub AiProvider);

impl FromStr for AiProviderArg {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input.parse::<AiProvider>().map(Self)
    }
}

#[derive(Debug, Clone)]
pub struct AiSessionModeArg(pub AiSessionMode);

impl FromStr for AiSessionModeArg {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input.parse::<AiSessionMode>().map(Self)
    }
}
