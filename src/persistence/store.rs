use crate::domain::config::AppConfig;
use crate::domain::review::ReviewSession;
use std::io::Error;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("invalid review name: {0}")]
    InvalidReviewName(String),
    #[error("review not found: {0}")]
    ReviewNotFound(String),
    #[error("io error: {0}")]
    Io(#[from] Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml deserialize error: {0}")]
    TomlDeserialize(#[from] toml::de::Error),
    #[error("toml serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn from_project_root(project_root: impl AsRef<Path>) -> Self {
        Self {
            root: project_root.as_ref().join(".parley"),
        }
    }

    /// # Errors
    ///
    /// Returns an error when the `.parley` review directories cannot be created.
    pub async fn ensure_dirs(&self) -> StoreResult<()> {
        fs::create_dir_all(self.reviews_dir()).await?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when the review name is invalid or the review cannot be written.
    pub async fn create_review(&self, session: &ReviewSession) -> StoreResult<()> {
        validate_review_name(&session.name)?;
        self.save_review(session).await
    }

    /// # Errors
    ///
    /// Returns an error when the review name is invalid, directories cannot be created, the session
    /// cannot be serialized, or the review file cannot be written.
    pub async fn save_review(&self, session: &ReviewSession) -> StoreResult<()> {
        validate_review_name(&session.name)?;
        self.ensure_dirs().await?;

        let path = self.review_path(&session.name)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let data = serde_json::to_vec_pretty(session)?;
        fs::write(path, data).await?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when the review name is invalid, the review is missing, or review data
    /// cannot be read or deserialized.
    pub async fn load_review(&self, name: &str) -> StoreResult<ReviewSession> {
        validate_review_name(name)?;
        let review_path = self.review_path(name)?;
        if let Some(review) = read_review_file(&review_path).await? {
            return Ok(review);
        }

        if let Some(review) = self.load_legacy_review(name).await? {
            return Ok(review);
        }

        Err(StoreError::ReviewNotFound(name.to_string()))
    }

    /// # Errors
    ///
    /// Returns an error when review directories cannot be read or persisted review files cannot be
    /// deserialized.
    pub async fn list_reviews(&self) -> StoreResult<Vec<String>> {
        self.ensure_dirs().await?;
        let mut dir = fs::read_dir(self.reviews_dir()).await?;
        let mut result = Vec::new();

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                let review_path = path.join("review.json");
                if let Some(review) = read_review_file(&review_path).await? {
                    result.push(review.name);
                }
            } else if let Some(name) = self.legacy_review_name(&path).await? {
                result.push(name);
            }
        }

        result.sort_unstable();
        result.dedup();
        Ok(result)
    }

    /// # Errors
    ///
    /// Returns an error when `review_name` is invalid.
    pub fn review_log_path(&self, review_name: &str) -> StoreResult<PathBuf> {
        validate_review_name(review_name)?;
        Ok(self.review_dir(review_name)?.join("logs").join("tui.log"))
    }

    fn review_path(&self, name: &str) -> StoreResult<PathBuf> {
        Ok(self.review_dir(name)?.join("review.json"))
    }

    fn review_dir(&self, name: &str) -> StoreResult<PathBuf> {
        validate_review_name(name)?;
        Ok(self.reviews_dir().join(normalize_review_name(name)?))
    }

    fn reviews_dir(&self) -> PathBuf {
        self.root.join("reviews")
    }

    /// # Errors
    ///
    /// Returns an error when config directories cannot be created or config data cannot be read or
    /// deserialized.
    pub async fn load_config(&self) -> StoreResult<AppConfig> {
        self.ensure_dirs().await?;
        let path = self.config_path();

        match fs::read(&path).await {
            Ok(bytes) => {
                let text = String::from_utf8(bytes).map_err(|error| {
                    StoreError::Io(Error::new(
                        ErrorKind::InvalidData,
                        format!("invalid utf-8 in config.toml: {error}"),
                    ))
                })?;
                Ok(toml::from_str(&text)?)
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(AppConfig::default()),
            Err(error) => Err(StoreError::Io(error)),
        }
    }

    /// # Errors
    ///
    /// Returns an error when config directories cannot be created, config data cannot be serialized,
    /// or the config file cannot be written.
    pub async fn save_config(&self, config: &AppConfig) -> StoreResult<()> {
        self.ensure_dirs().await?;
        let data = toml::to_string_pretty(config)?;
        fs::write(self.config_path(), data).await?;
        Ok(())
    }

    fn config_path(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    // Legacy compatibility for flat review files that predate per-review directories.
    async fn load_legacy_review(&self, name: &str) -> StoreResult<Option<ReviewSession>> {
        let legacy_path = self.legacy_review_path(name)?;
        read_review_file(&legacy_path).await
    }

    async fn legacy_review_name(&self, path: &Path) -> StoreResult<Option<String>> {
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            return Ok(None);
        }

        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            return Ok(None);
        };

        let normalized_path = self.review_path(stem)?;
        if fs::try_exists(normalized_path).await? {
            Ok(None)
        } else {
            Ok(Some(stem.to_string()))
        }
    }

    fn legacy_review_path(&self, name: &str) -> StoreResult<PathBuf> {
        validate_review_name(name)?;
        Ok(self.reviews_dir().join(format!("{name}.json")))
    }
}

async fn read_review_file(path: &Path) -> StoreResult<Option<ReviewSession>> {
    match fs::read(path).await {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(StoreError::Io(error)),
    }
}

/// # Errors
///
/// Returns an error when the review name is empty after trimming or contains unsupported
/// characters.
pub fn normalize_review_name(name: &str) -> StoreResult<String> {
    validate_review_name(name)?;
    let normalized = name.trim_matches(|ch| matches!(ch, '_' | '.')).to_string();
    if normalized.is_empty() {
        return Err(StoreError::InvalidReviewName(name.to_string()));
    }
    Ok(normalized)
}

/// # Errors
///
/// Returns an error when the review name is empty or contains characters other than ASCII
/// alphanumerics, `.`, `_`, or `-`.
pub fn validate_review_name(name: &str) -> StoreResult<()> {
    if name.is_empty() {
        return Err(StoreError::InvalidReviewName(name.to_string()));
    }

    if name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        Ok(())
    } else {
        Err(StoreError::InvalidReviewName(name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::config::{AiConfig, DiffViewMode};
    use anyhow::Result;
    use tempfile::tempdir;

    #[tokio::test]
    async fn save_and_load_review_should_round_trip() -> Result<()> {
        let tmp = tempdir()?;
        let store = super::Store::from_project_root(tmp.path());
        let review = super::ReviewSession::new("r1".into(), 1);

        store.save_review(&review).await?;
        let loaded = store.load_review("r1").await?;

        assert_eq!(loaded.name, "r1");
        assert_eq!(loaded.state, review.state);
        Ok(())
    }

    #[tokio::test]
    async fn save_review_should_use_normalized_review_directory() -> Result<()> {
        let tmp = tempdir()?;
        let store = super::Store::from_project_root(tmp.path());
        let review = super::ReviewSession::new("__r1__".into(), 1);

        store.save_review(&review).await?;

        let path = tmp.path().join(".parley/reviews/r1/review.json");
        assert!(path.exists());
        Ok(())
    }

    #[tokio::test]
    async fn load_and_list_reviews_should_support_legacy_flat_files() -> Result<()> {
        let tmp = tempdir()?;
        let store = super::Store::from_project_root(tmp.path());
        store.ensure_dirs().await?;
        let review = super::ReviewSession::new("legacy".into(), 1);
        let data = serde_json::to_vec_pretty(&review)?;
        tokio::fs::write(tmp.path().join(".parley/reviews/legacy.json"), data).await?;

        let loaded = store.load_review("legacy").await?;
        let reviews = store.list_reviews().await?;

        assert_eq!(loaded.name, "legacy");
        assert_eq!(reviews, vec!["legacy"]);
        Ok(())
    }

    #[test]
    fn validate_review_name_should_reject_slash() {
        let result = super::validate_review_name("bad/name");

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn save_and_load_config_should_round_trip() -> Result<()> {
        let tmp = tempdir()?;
        let store = super::Store::from_project_root(tmp.path());
        let config = super::AppConfig {
            user_name: "User".to_string(),
            theme: "nord".to_string(),
            diff_view: DiffViewMode::Unified,
            ignore_parley_dir: true,
            log_level: "debug".to_string(),
            ai: AiConfig::default(),
        };

        store.save_config(&config).await?;
        let loaded = store.load_config().await?;

        assert_eq!(loaded, config);
        Ok(())
    }

    #[tokio::test]
    async fn load_config_should_support_legacy_name_field() -> Result<()> {
        let tmp = tempdir()?;
        let store = super::Store::from_project_root(tmp.path());
        store.ensure_dirs().await?;

        super::fs::write(
            tmp.path().join(".parley").join("config.toml"),
            "name = \"User\"\ntheme = \"nord\"\n",
        )
        .await?;

        let loaded = store.load_config().await?;

        assert_eq!(loaded.user_name, "User");
        assert_eq!(loaded.theme, "nord");
        assert_eq!(loaded.diff_view, DiffViewMode::SideBySide);
        assert!(loaded.ignore_parley_dir);
        assert_eq!(loaded.log_level, "info");
        Ok(())
    }
}
