use std::path::{Path, PathBuf};

use tokio::fs;

use crate::domain::{config::AppConfig, review::ReviewSession};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("invalid review name: {0}")]
    InvalidReviewName(String),
    #[error("review not found: {0}")]
    ReviewNotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
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
            root: project_root.as_ref().join(".parlar"),
        }
    }

    pub async fn ensure_dirs(&self) -> StoreResult<()> {
        fs::create_dir_all(self.reviews_dir()).await?;
        Ok(())
    }

    pub async fn create_review(&self, session: &ReviewSession) -> StoreResult<()> {
        validate_review_name(&session.name)?;
        self.save_review(session).await
    }

    pub async fn save_review(&self, session: &ReviewSession) -> StoreResult<()> {
        validate_review_name(&session.name)?;
        self.ensure_dirs().await?;

        let path = self.review_path(&session.name)?;
        let data = serde_json::to_vec_pretty(session)?;
        fs::write(path, data).await?;
        Ok(())
    }

    pub async fn load_review(&self, name: &str) -> StoreResult<ReviewSession> {
        validate_review_name(name)?;
        let path = self.review_path(name)?;

        match fs::read(&path).await {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(StoreError::ReviewNotFound(name.to_string()))
            }
            Err(error) => Err(StoreError::Io(error)),
        }
    }

    pub async fn list_reviews(&self) -> StoreResult<Vec<String>> {
        self.ensure_dirs().await?;
        let mut dir = fs::read_dir(self.reviews_dir()).await?;
        let mut result = Vec::new();

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("json")
                && let Some(stem) = path.file_stem().and_then(|value| value.to_str())
            {
                result.push(stem.to_string());
            }
        }

        result.sort_unstable();
        Ok(result)
    }

    pub async fn load_config(&self) -> StoreResult<AppConfig> {
        self.ensure_dirs().await?;
        let path = self.config_path();

        match fs::read(&path).await {
            Ok(bytes) => {
                let text = String::from_utf8(bytes).map_err(|error| {
                    StoreError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid utf-8 in config.toml: {error}"),
                    ))
                })?;
                Ok(toml::from_str(&text)?)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.load_legacy_json_config().await
            }
            Err(error) => Err(StoreError::Io(error)),
        }
    }

    pub async fn save_config(&self, config: &AppConfig) -> StoreResult<()> {
        self.ensure_dirs().await?;
        let data = toml::to_string_pretty(config)?;
        fs::write(self.config_path(), data).await?;
        Ok(())
    }

    fn review_path(&self, name: &str) -> StoreResult<PathBuf> {
        validate_review_name(name)?;
        Ok(self.reviews_dir().join(format!("{name}.json")))
    }

    fn reviews_dir(&self) -> PathBuf {
        self.root.join("reviews")
    }

    fn config_path(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    fn legacy_config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    async fn load_legacy_json_config(&self) -> StoreResult<AppConfig> {
        match fs::read(self.legacy_config_path()).await {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig::default()),
            Err(error) => Err(StoreError::Io(error)),
        }
    }
}

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
    use tempfile::tempdir;
    use tokio::fs;

    use crate::domain::{config::AppConfig, review::ReviewSession};

    use super::{Store, validate_review_name};

    #[tokio::test]
    async fn save_and_load_review_should_round_trip() {
        let tmp = tempdir().expect("tempdir should exist");
        let store = Store::from_project_root(tmp.path());
        let review = ReviewSession::new("r1".into(), 1);

        store
            .save_review(&review)
            .await
            .expect("review should save successfully");
        let loaded = store
            .load_review("r1")
            .await
            .expect("review should load successfully");

        assert_eq!(loaded.name, "r1");
        assert_eq!(loaded.state, review.state);
    }

    #[test]
    fn validate_review_name_should_reject_slash() {
        let result = validate_review_name("bad/name");

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn save_and_load_config_should_round_trip() {
        let tmp = tempdir().expect("tempdir should exist");
        let store = Store::from_project_root(tmp.path());
        let config = AppConfig {
            user_name: "Vic".to_string(),
            theme: "nord".to_string(),
        };

        store
            .save_config(&config)
            .await
            .expect("config should save successfully");
        let loaded = store
            .load_config()
            .await
            .expect("config should load successfully");

        assert_eq!(loaded, config);
    }

    #[tokio::test]
    async fn load_config_should_support_legacy_name_field() {
        let tmp = tempdir().expect("tempdir should exist");
        let store = Store::from_project_root(tmp.path());
        store
            .ensure_dirs()
            .await
            .expect("store dirs should be created");

        fs::write(
            tmp.path().join(".parlar").join("config.toml"),
            "name = \"Vic\"\ntheme = \"nord\"\n",
        )
        .await
        .expect("legacy config should be written");

        let loaded = store
            .load_config()
            .await
            .expect("legacy config should load successfully");

        assert_eq!(loaded.user_name, "Vic");
        assert_eq!(loaded.theme, "nord");
    }
}
