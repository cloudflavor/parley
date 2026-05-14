use crate::domain::config::AppConfig;
use crate::domain::review::ReviewSession;
use std::env;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
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
    #[error("could not resolve $HOME for global parley storage")]
    HomeNotFound,
    #[error("local .parley path exists but is not a directory: {0}")]
    LocalStorePathNotDirectory(PathBuf),
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

    #[must_use]
    pub fn from_storage_root(storage_root: impl AsRef<Path>) -> Self {
        Self {
            root: storage_root.as_ref().to_path_buf(),
        }
    }

    /// # Errors
    ///
    /// Returns an error when global storage cannot be resolved or an existing local `.parley`
    /// marker is not a directory.
    pub async fn resolve_from_context(
        ctx: &crate::git::worktree::RepositoryContext,
    ) -> StoreResult<Self> {
        let global_root = default_global_root()?;
        Self::resolve_with_global_root(&ctx.storage_root, global_root).await
    }

    /// # Errors
    ///
    /// Returns an error when global storage cannot be resolved or an existing local `.parley`
    /// marker is not a directory.
    pub async fn resolve(project_root: impl AsRef<Path>) -> StoreResult<Self> {
        let global_root = default_global_root()?;
        Self::resolve_with_global_root(project_root, global_root).await
    }

    /// # Errors
    ///
    /// Returns an error when an existing local `.parley` marker is not a directory.
    pub async fn resolve_with_global_root(
        project_root: impl AsRef<Path>,
        global_root: impl AsRef<Path>,
    ) -> StoreResult<Self> {
        let project_root = project_root.as_ref();
        let local_root = project_root.join(".parley");
        match fs::metadata(&local_root).await {
            Ok(metadata) if metadata.is_dir() => return Ok(Self { root: local_root }),
            Ok(_) => return Err(StoreError::LocalStorePathNotDirectory(local_root)),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(StoreError::Io(error)),
        }

        Ok(Self {
            root: global_root
                .as_ref()
                .join("repos")
                .join(repo_storage_name(project_root).await?),
        })
    }

    #[must_use]
    pub fn root_path(&self) -> &Path {
        &self.root
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
        self.save_review(session).await
    }

    /// # Errors
    ///
    /// Returns an error when the review name is invalid, directories cannot be created, the session
    /// cannot be serialized, or the review file cannot be written.
    pub async fn save_review(&self, session: &ReviewSession) -> StoreResult<()> {
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
        Ok(self.review_dir(review_name)?.join("logs").join("tui.log"))
    }

    fn review_path(&self, name: &str) -> StoreResult<PathBuf> {
        Ok(self.review_dir(name)?.join("review.json"))
    }

    fn review_dir(&self, name: &str) -> StoreResult<PathBuf> {
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

        let Some(bytes) = read_optional_file(&path).await? else {
            return Ok(AppConfig::default());
        };
        let text = String::from_utf8(bytes).map_err(|error| {
            StoreError::Io(Error::new(
                ErrorKind::InvalidData,
                format!("invalid utf-8 in config.toml: {error}"),
            ))
        })?;
        Ok(toml::from_str(&text)?)
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
    let Some(bytes) = read_optional_file(path).await? else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_slice(&bytes)?))
}

async fn read_optional_file(path: &Path) -> StoreResult<Option<Vec<u8>>> {
    match fs::read(path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(StoreError::Io(error)),
    }
}

fn default_global_root() -> StoreResult<PathBuf> {
    let home = env::var_os("HOME").ok_or(StoreError::HomeNotFound)?;
    Ok(PathBuf::from(home).join(".config").join("parley"))
}

async fn repo_storage_name(project_root: &Path) -> StoreResult<String> {
    let canonical_root = fs::canonicalize(project_root).await?;
    let repo_name = canonical_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(normalize_path_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "repository".to_string());

    Ok(format!(
        "{repo_name}-{:016x}",
        stable_path_hash(&canonical_root)
    ))
}

fn stable_path_hash(path: &Path) -> u64 {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}

fn normalize_path_component(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut previous_was_separator = false;

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            output.push(ch);
            previous_was_separator = false;
            continue;
        }

        if !previous_was_separator && !output.is_empty() {
            output.push('_');
            previous_was_separator = true;
        }
    }

    output
        .trim_matches(|ch| matches!(ch, '_' | '.'))
        .to_string()
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
    use crate::domain::review::{
        Author, DiffSide, NewLineComment, SourceAnchorSnapshot, StoredAnchorSnapshot,
    };
    use anyhow::Result;
    use tempfile::tempdir;
    use tokio::fs as tokio_fs;

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
    async fn resolve_should_prefer_existing_local_store() -> Result<()> {
        let tmp = tempdir()?;
        let global = tempdir()?;
        tokio_fs::create_dir(tmp.path().join(".parley")).await?;

        let store = super::Store::resolve_with_global_root(tmp.path(), global.path()).await?;

        assert_eq!(store.root_path(), tmp.path().join(".parley"));
        Ok(())
    }

    #[tokio::test]
    async fn resolve_should_use_global_repo_named_store_without_local_marker() -> Result<()> {
        let tmp = tempdir()?;
        let global = tempdir()?;

        let store = super::Store::resolve_with_global_root(tmp.path(), global.path()).await?;

        let expected = global
            .path()
            .join("repos")
            .join(super::repo_storage_name(tmp.path()).await?);
        assert_eq!(store.root_path(), expected);
        assert!(!tokio_fs::try_exists(tmp.path().join(".parley")).await?);
        Ok(())
    }

    #[tokio::test]
    async fn resolve_should_reject_local_store_file() -> Result<()> {
        let tmp = tempdir()?;
        let global = tempdir()?;
        tokio_fs::write(tmp.path().join(".parley"), "").await?;

        let result = super::Store::resolve_with_global_root(tmp.path(), global.path()).await;

        assert!(matches!(
            result,
            Err(super::StoreError::LocalStorePathNotDirectory(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn save_review_should_use_normalized_review_directory() -> Result<()> {
        let tmp = tempdir()?;
        let store = super::Store::from_project_root(tmp.path());
        let review = super::ReviewSession::new("__r1__".into(), 1);

        store.save_review(&review).await?;

        let path = tmp.path().join(".parley/reviews/r1/review.json");
        assert!(tokio_fs::try_exists(path).await?);
        Ok(())
    }

    #[tokio::test]
    async fn load_and_list_reviews_should_support_legacy_flat_files() -> Result<()> {
        let tmp = tempdir()?;
        let store = super::Store::from_project_root(tmp.path());
        store.ensure_dirs().await?;
        let review = super::ReviewSession::new("legacy".into(), 1);
        let data = serde_json::to_vec_pretty(&review)?;
        tokio_fs::write(tmp.path().join(".parley/reviews/legacy.json"), data).await?;

        let loaded = store.load_review("legacy").await?;
        let reviews = store.list_reviews().await?;

        assert_eq!(loaded.name, "legacy");
        assert_eq!(reviews, vec!["legacy"]);
        Ok(())
    }

    #[tokio::test]
    async fn load_review_should_default_missing_original_anchor() -> Result<()> {
        let tmp = tempdir()?;
        let store = super::Store::from_project_root(tmp.path());
        store.ensure_dirs().await?;
        let review_dir = tmp.path().join(".parley/reviews/old");
        tokio_fs::create_dir_all(&review_dir).await?;
        tokio_fs::write(
            review_dir.join("review.json"),
            r#"{
  "name": "old",
  "state": "open",
  "created_at_ms": 1,
  "updated_at_ms": 1,
  "comments": [
    {
      "id": 1,
      "file_path": "src/lib.rs",
      "old_line": null,
      "new_line": 1,
      "line_range": null,
      "side": "right",
      "line_anchor": null,
      "detached": false,
      "body": "old",
      "author": "user",
      "status": "open",
      "replies": [],
      "created_at_ms": 1,
      "updated_at_ms": 1,
      "addressed_at_ms": null
    }
  ],
  "next_comment_id": 2,
  "next_reply_id": 1
}"#,
        )
        .await?;

        let loaded = store.load_review("old").await?;

        assert_eq!(loaded.comments[0].original_anchor, None);
        Ok(())
    }

    #[tokio::test]
    async fn save_and_load_review_should_round_trip_original_anchor() -> Result<()> {
        let tmp = tempdir()?;
        let store = super::Store::from_project_root(tmp.path());
        let mut review = super::ReviewSession::new("anchored".into(), 1);
        let original_anchor = StoredAnchorSnapshot {
            file_path: "src/lib.rs".into(),
            side: DiffSide::Right,
            old_line: None,
            new_line: Some(10),
            line_range: None,
            selected_text: "let value = 1;".into(),
            before_context: vec!["fn main() {".into()],
            after_context: vec!["}".into()],
            diff: None,
            source: Some(SourceAnchorSnapshot {
                file_content_hash: Some("file-hash".into()),
                selected_text_hash: Some("text-hash".into()),
            }),
            base_rev: Some("base".into()),
            head_rev: Some("head".into()),
        };
        review.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(10),
                line_range: None,
                side: DiffSide::Right,
                line_anchor: None,
                original_anchor: Some(original_anchor.clone()),
                body: "anchor".into(),
                author: Author::User,
            },
            2,
        );

        store.save_review(&review).await?;
        let loaded = store.load_review("anchored").await?;

        assert_eq!(loaded.comments[0].original_anchor, Some(original_anchor));
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
            last_worktree: None,
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

        tokio_fs::write(
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
