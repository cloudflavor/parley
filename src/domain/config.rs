use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    #[serde(alias = "name", default = "default_user_name")]
    pub user_name: String,
    pub theme: String,
}

pub fn default_user_name() -> String {
    std::env::var("PARLAR_USER_NAME")
        .ok()
        .or_else(|| std::env::var("USER").ok())
        .or_else(|| std::env::var("USERNAME").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "User".to_string())
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            user_name: default_user_name(),
            theme: "default".to_string(),
        }
    }
}
