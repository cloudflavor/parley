use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    Codex,
    Claude,
    Opencode,
}

impl AiProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Opencode => "opencode",
        }
    }
}

impl FromStr for AiProvider {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            "opencode" => Ok(Self::Opencode),
            _ => Err(format!("invalid ai provider: {input}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiSessionMode {
    Reply,
    Refactor,
}

impl AiSessionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reply => "reply",
            Self::Refactor => "refactor",
        }
    }
}

impl FromStr for AiSessionMode {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "reply" => Ok(Self::Reply),
            "refactor" => Ok(Self::Refactor),
            _ => Err(format!("invalid ai session mode: {input}")),
        }
    }
}
