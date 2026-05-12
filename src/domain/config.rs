use crate::domain::ai::{AiProvider, AiSessionMode};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    #[serde(alias = "name", default = "default_user_name")]
    pub user_name: String,
    pub theme: String,
    pub diff_view: DiffViewMode,
    #[serde(default = "default_ignore_parley_dir")]
    pub ignore_parley_dir: bool,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    pub ai: AiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DiffViewMode {
    #[default]
    SideBySide,
    Unified,
}

impl DiffViewMode {
    #[must_use]
    pub fn is_side_by_side(&self) -> bool {
        matches!(self, Self::SideBySide)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AgentTransport {
    #[default]
    Acp,
    Cli,
    PiRpc,
}

impl AgentTransport {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Acp => "acp",
            Self::Cli => "cli",
            Self::PiRpc => "pi_rpc",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AiProviderConfig {
    pub transport: AgentTransport,
    #[serde(alias = "program")]
    pub client: String,
    pub model: Option<String>,
    pub model_arg: Option<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AiConfig {
    pub timeout_seconds: u64,
    pub default_provider: AiProvider,
    pub default_transport: Option<AgentTransport>,
    pub prompt_path: Option<String>,
    pub reply_prompt_path: Option<String>,
    pub refactor_prompt_path: Option<String>,
    pub codex: AiProviderConfig,
    pub claude: AiProviderConfig,
    pub opencode: AiProviderConfig,
    pub pi: AiProviderConfig,
}

#[must_use]
pub fn default_user_name() -> String {
    std::env::var("PARLEY_USER_NAME")
        .ok()
        .or_else(|| std::env::var("USER").ok())
        .or_else(|| std::env::var("USERNAME").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "User".to_string())
}

#[must_use]
pub fn default_log_level() -> String {
    "info".to_string()
}

#[must_use]
pub fn default_ignore_parley_dir() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            user_name: default_user_name(),
            theme: "default".to_string(),
            diff_view: DiffViewMode::default(),
            ignore_parley_dir: default_ignore_parley_dir(),
            log_level: default_log_level(),
            ai: AiConfig::default(),
        }
    }
}

impl Default for AiProviderConfig {
    fn default() -> Self {
        Self {
            transport: AgentTransport::Acp,
            client: String::new(),
            model: None,
            model_arg: Some("--model".to_string()),
            args: Vec::new(),
        }
    }
}

impl AiProviderConfig {
    #[must_use]
    pub fn with_client(client: &str) -> Self {
        Self {
            client: client.to_string(),
            model: None,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn command_label(&self) -> String {
        let mut parts = Vec::with_capacity(self.args.len().saturating_add(1));
        parts.push(self.client.as_str());
        parts.extend(self.args.iter().map(String::as_str));
        parts.join(" ")
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 120,
            default_provider: AiProvider::Opencode,
            default_transport: Some(AgentTransport::Acp),
            prompt_path: None,
            reply_prompt_path: None,
            refactor_prompt_path: None,
            codex: default_provider_config_for_transport(AiProvider::Codex, AgentTransport::Acp)
                .expect("codex acp profile should exist"),
            claude: default_provider_config_for_transport(AiProvider::Claude, AgentTransport::Acp)
                .expect("claude acp profile should exist"),
            opencode: default_provider_config_for_transport(
                AiProvider::Opencode,
                AgentTransport::Acp,
            )
            .expect("opencode acp profile should exist"),
            pi: default_provider_config_for_transport(AiProvider::Pi, AgentTransport::PiRpc)
                .expect("pi rpc profile should exist"),
        }
    }
}

impl AiConfig {
    #[must_use]
    pub fn provider_config(&self, provider: AiProvider) -> &AiProviderConfig {
        match provider {
            AiProvider::Codex => &self.codex,
            AiProvider::Claude => &self.claude,
            AiProvider::Opencode => &self.opencode,
            AiProvider::Pi => &self.pi,
        }
    }

    #[must_use]
    pub fn provider_config_for_transport(
        &self,
        provider: AiProvider,
        transport: Option<AgentTransport>,
    ) -> AiProviderConfig {
        let configured = self.provider_config(provider);
        match transport {
            Some(AgentTransport::Acp)
                if configured.transport != AgentTransport::Acp
                    || is_cli_command_for_acp_transport(provider, configured) =>
            {
                default_provider_config_for_transport(provider, AgentTransport::Acp)
                    .unwrap_or_else(|| configured.clone())
            }
            Some(AgentTransport::Cli) if configured.transport != AgentTransport::Cli => {
                default_provider_config_for_transport(provider, AgentTransport::Cli)
                    .unwrap_or_else(|| configured.clone())
            }
            Some(AgentTransport::PiRpc) if configured.transport != AgentTransport::PiRpc => {
                default_provider_config_for_transport(provider, AgentTransport::PiRpc)
                    .unwrap_or_else(|| configured.clone())
            }
            _ => configured.clone(),
        }
    }

    #[must_use]
    pub fn prompt_path_for_mode(&self, mode: AiSessionMode) -> Option<&str> {
        let mode_path = match mode {
            AiSessionMode::Reply => self.reply_prompt_path.as_deref(),
            AiSessionMode::Refactor => self.refactor_prompt_path.as_deref(),
        };
        mode_path
            .or(self.prompt_path.as_deref())
            .map(str::trim)
            .filter(|path| !path.is_empty())
    }
}

#[derive(Debug, Clone, Copy)]
struct ProviderCommandProfile {
    transport: AgentTransport,
    client: &'static str,
    args: &'static [&'static str],
    model_arg: Option<&'static str>,
}

impl ProviderCommandProfile {
    fn to_config(self) -> AiProviderConfig {
        let mut config = AiProviderConfig::with_client(self.client);
        config.transport = self.transport;
        config.args = self.args.iter().map(|value| (*value).to_string()).collect();
        config.model_arg = self.model_arg.map(str::to_string);
        config
    }

    fn command_label(self) -> String {
        let mut parts = Vec::with_capacity(self.args.len().saturating_add(1));
        parts.push(self.client);
        parts.extend(self.args);
        parts.join(" ")
    }
}

fn provider_command_profile(
    provider: AiProvider,
    transport: AgentTransport,
) -> Option<ProviderCommandProfile> {
    match (provider, transport) {
        (AiProvider::Codex, AgentTransport::Acp) => Some(ProviderCommandProfile {
            transport: AgentTransport::Acp,
            client: "codex-acp",
            args: &[],
            model_arg: Some("--model"),
        }),
        (AiProvider::Codex, AgentTransport::Cli) => Some(ProviderCommandProfile {
            transport: AgentTransport::Cli,
            client: "codex",
            args: &["exec"],
            model_arg: Some("--model"),
        }),
        (AiProvider::Claude, AgentTransport::Acp) => Some(ProviderCommandProfile {
            transport: AgentTransport::Acp,
            client: "claude-agent-acp",
            args: &[],
            model_arg: Some("--model"),
        }),
        (AiProvider::Claude, AgentTransport::Cli) => Some(ProviderCommandProfile {
            transport: AgentTransport::Cli,
            client: "claude",
            args: &["-p"],
            model_arg: Some("--model"),
        }),
        (AiProvider::Opencode, AgentTransport::Acp) => Some(ProviderCommandProfile {
            transport: AgentTransport::Acp,
            client: "opencode",
            args: &["acp"],
            model_arg: Some("-m"),
        }),
        (AiProvider::Opencode, AgentTransport::Cli) => Some(ProviderCommandProfile {
            transport: AgentTransport::Cli,
            client: "opencode",
            args: &["run"],
            model_arg: Some("-m"),
        }),
        (AiProvider::Pi, AgentTransport::PiRpc) => Some(ProviderCommandProfile {
            transport: AgentTransport::PiRpc,
            client: "pi",
            args: &["--mode", "rpc", "--no-session"],
            model_arg: Some("--model"),
        }),
        _ => None,
    }
}

fn default_provider_config_for_transport(
    provider: AiProvider,
    transport: AgentTransport,
) -> Option<AiProviderConfig> {
    provider_command_profile(provider, transport)
        .or_else(|| {
            if provider == AiProvider::Pi && transport == AgentTransport::Acp {
                provider_command_profile(provider, AgentTransport::PiRpc)
            } else {
                None
            }
        })
        .map(ProviderCommandProfile::to_config)
}

#[must_use]
pub fn acp_command_replacement(provider: AiProvider, config: &AiProviderConfig) -> Option<String> {
    if is_cli_command_for_acp_transport(provider, config) {
        provider_command_profile(provider, AgentTransport::Acp)
            .map(ProviderCommandProfile::command_label)
    } else {
        None
    }
}

fn is_cli_command_for_acp_transport(provider: AiProvider, config: &AiProviderConfig) -> bool {
    if config.transport != AgentTransport::Acp {
        return false;
    }
    let client = Path::new(&config.client)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(config.client.as_str());
    match provider {
        AiProvider::Codex => client == "codex",
        AiProvider::Claude => client == "claude" || client == "claude-code",
        AiProvider::Opencode => {
            client == "opencode" && config.args.first().map(String::as_str) != Some("acp")
        }
        AiProvider::Pi => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentTransport, AiConfig, AppConfig};
    use crate::domain::ai::AiSessionMode;
    use anyhow::Result;

    #[test]
    fn default_config_ignores_parley_dir() {
        let config = AppConfig::default();

        assert!(config.ignore_parley_dir);
    }

    #[test]
    fn ai_prompt_path_for_mode_prefers_mode_specific_path() {
        let config = AiConfig {
            prompt_path: Some("prompts/default.md".to_string()),
            reply_prompt_path: Some("prompts/reply.md".to_string()),
            refactor_prompt_path: None,
            ..AiConfig::default()
        };

        assert_eq!(
            config.prompt_path_for_mode(AiSessionMode::Reply),
            Some("prompts/reply.md")
        );
        assert_eq!(
            config.prompt_path_for_mode(AiSessionMode::Refactor),
            Some("prompts/default.md")
        );
    }

    #[test]
    fn default_ai_config_uses_persistent_agent_transports() {
        let config = AiConfig::default();

        assert_eq!(config.codex.transport, AgentTransport::Acp);
        assert_eq!(config.default_transport, Some(AgentTransport::Acp));
        assert_eq!(config.claude.transport, AgentTransport::Acp);
        assert_eq!(config.opencode.transport, AgentTransport::Acp);
        assert_eq!(config.pi.transport, AgentTransport::PiRpc);
        assert_eq!(config.opencode.args, vec!["acp"]);
        assert_eq!(config.pi.args, vec!["--mode", "rpc", "--no-session"]);
    }

    #[test]
    fn provider_config_for_transport_uses_builtin_cli_profiles() {
        let config = AiConfig::default();

        let codex = config.provider_config_for_transport(
            crate::domain::ai::AiProvider::Codex,
            Some(AgentTransport::Cli),
        );
        let opencode = config.provider_config_for_transport(
            crate::domain::ai::AiProvider::Opencode,
            Some(AgentTransport::Cli),
        );

        assert_eq!(codex.transport, AgentTransport::Cli);
        assert_eq!(codex.client, "codex");
        assert_eq!(codex.args, vec!["exec"]);
        assert_eq!(opencode.transport, AgentTransport::Cli);
        assert_eq!(opencode.client, "opencode");
        assert_eq!(opencode.args, vec!["run"]);
    }

    #[test]
    fn provider_config_for_transport_repairs_cli_command_for_acp_transport() {
        let mut config = AiConfig::default();
        config.opencode.transport = AgentTransport::Acp;
        config.opencode.client = "opencode".to_string();
        config.opencode.args = vec!["run".to_string()];

        let opencode = config.provider_config_for_transport(
            crate::domain::ai::AiProvider::Opencode,
            Some(AgentTransport::Acp),
        );

        assert_eq!(opencode.transport, AgentTransport::Acp);
        assert_eq!(opencode.client, "opencode");
        assert_eq!(opencode.args, vec!["acp"]);
    }

    #[test]
    fn app_config_deserializes_custom_prompt_paths() -> Result<()> {
        let config: AppConfig = toml::from_str(
            r#"
            [ai]
            prompt_path = "prompts/shared.md"
            reply_prompt_path = "prompts/reply.md"
            refactor_prompt_path = "prompts/refactor.md"
            "#,
        )?;

        assert_eq!(config.ai.prompt_path.as_deref(), Some("prompts/shared.md"));
        assert_eq!(
            config.ai.reply_prompt_path.as_deref(),
            Some("prompts/reply.md")
        );
        assert_eq!(
            config.ai.refactor_prompt_path.as_deref(),
            Some("prompts/refactor.md")
        );
        Ok(())
    }
}
