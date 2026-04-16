use serde::{Deserialize, Serialize};

use crate::domain::ai::AiProvider;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    #[serde(alias = "name", default = "default_user_name")]
    pub user_name: String,
    pub theme: String,
    pub diff_view: DiffViewMode,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    pub ai: AiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffViewMode {
    SideBySide,
    Unified,
}

impl DiffViewMode {
    pub fn is_side_by_side(&self) -> bool {
        matches!(self, Self::SideBySide)
    }
}

impl Default for DiffViewMode {
    fn default() -> Self {
        Self::SideBySide
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptTransport {
    Stdin,
    Argv,
}

impl Default for PromptTransport {
    fn default() -> Self {
        Self::Stdin
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AiProviderConfig {
    #[serde(alias = "program")]
    pub client: String,
    pub model: Option<String>,
    pub model_arg: Option<String>,
    pub args: Vec<String>,
    pub prompt_transport: PromptTransport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AiConfig {
    pub timeout_seconds: u64,
    pub default_provider: AiProvider,
    pub codex: AiProviderConfig,
    pub claude: AiProviderConfig,
    pub opencode: AiProviderConfig,
}

pub fn default_user_name() -> String {
    std::env::var("PARLAR_USER_NAME")
        .ok()
        .or_else(|| std::env::var("USER").ok())
        .or_else(|| std::env::var("USERNAME").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "User".to_string())
}

pub fn default_log_level() -> String {
    "info".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            user_name: default_user_name(),
            theme: "default".to_string(),
            diff_view: DiffViewMode::default(),
            log_level: default_log_level(),
            ai: AiConfig::default(),
        }
    }
}

impl Default for AiProviderConfig {
    fn default() -> Self {
        Self {
            client: String::new(),
            model: None,
            model_arg: Some("--model".to_string()),
            args: Vec::new(),
            prompt_transport: PromptTransport::Stdin,
        }
    }
}

impl AiProviderConfig {
    pub fn with_client(client: &str, model: Option<&str>) -> Self {
        Self {
            client: client.to_string(),
            model: model.map(ToString::to_string),
            ..Self::default()
        }
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        let mut codex = AiProviderConfig::with_client("codex", Some("gpt-5"));
        codex.args = vec!["exec".to_string()];
        codex.prompt_transport = PromptTransport::Argv;

        let mut claude = AiProviderConfig::with_client("claude", Some("claude-sonnet-4"));
        claude.args = vec!["-p".to_string()];
        claude.prompt_transport = PromptTransport::Argv;

        let mut opencode = AiProviderConfig::with_client("opencode", Some("kimi-k2.5:cloud"));
        opencode.args = vec!["run".to_string()];
        opencode.model_arg = Some("-m".to_string());
        opencode.prompt_transport = PromptTransport::Argv;
        Self {
            timeout_seconds: 120,
            default_provider: AiProvider::Opencode,
            codex,
            claude,
            opencode,
        }
    }
}

impl AiConfig {
    pub fn provider_config(&self, provider: AiProvider) -> &AiProviderConfig {
        match provider {
            AiProvider::Codex => &self.codex,
            AiProvider::Claude => &self.claude,
            AiProvider::Opencode => &self.opencode,
        }
    }
}
