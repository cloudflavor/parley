use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};

use crate::domain::config::AppConfig;
use crate::domain::review::{
    Author, CommentStatus, DiffSide, LineAnchorSnapshot, NewLineComment, ReanchorLineComment,
    ReviewSession, ReviewState,
};
use crate::persistence::store::{Store, StoreError};
use crate::utils::time::now_ms;

#[derive(Debug, Clone)]
pub struct ReviewService {
    store: Store,
}

#[derive(Debug, Clone)]
pub struct AddCommentInput {
    pub file_path: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub side: DiffSide,
    pub line_anchor: Option<LineAnchorSnapshot>,
    pub body: String,
    pub author: Author,
}

#[derive(Debug, Clone)]
pub struct AddReplyInput {
    pub comment_id: u64,
    pub author: Author,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct ReanchorCommentInput {
    pub comment_id: u64,
    pub file_path: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub side: DiffSide,
    pub line_anchor: Option<LineAnchorSnapshot>,
}

impl ReviewService {
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    /// # Errors
    ///
    /// Returns an error when the clock is invalid or the review cannot be persisted.
    pub async fn create_review(&self, name: &str) -> Result<ReviewSession> {
        let session = ReviewSession::new(name.to_string(), now_ms()?);
        self.store
            .create_review(&session)
            .await
            .with_context(|| format!("failed to create review {name}"))?;
        Ok(session)
    }

    /// # Errors
    ///
    /// Returns an error when the review cannot be loaded from storage.
    pub async fn load_review(&self, name: &str) -> Result<ReviewSession> {
        self.store
            .load_review(name)
            .await
            .with_context(|| format!("failed to load review {name}"))
    }

    /// # Errors
    ///
    /// Returns an error when the review exists but cannot be loaded, or when a missing review
    /// cannot be created.
    pub async fn load_or_create_review(&self, name: &str) -> Result<ReviewSession> {
        match self.store.load_review(name).await {
            Ok(session) => Ok(session),
            Err(StoreError::ReviewNotFound(_)) => self.create_review(name).await,
            Err(error) => Err(error).with_context(|| format!("failed to load review {name}")),
        }
    }

    /// # Errors
    ///
    /// Returns an error when review storage cannot be listed.
    pub async fn list_reviews(&self) -> Result<Vec<String>> {
        self.store
            .list_reviews()
            .await
            .context("failed to list reviews")
    }

    /// # Errors
    ///
    /// Returns an error when configuration cannot be loaded from storage.
    pub async fn load_config(&self) -> Result<AppConfig> {
        self.store
            .load_config()
            .await
            .context("failed to load parler config")
    }

    /// # Errors
    ///
    /// Returns an error when configuration cannot be saved to storage.
    pub async fn save_config(&self, config: &AppConfig) -> Result<()> {
        self.store
            .save_config(config)
            .await
            .context("failed to save parler config")
    }

    /// # Errors
    ///
    /// Returns an error when `review_name` is invalid.
    pub fn review_log_path(&self, review_name: &str) -> Result<PathBuf> {
        self.store
            .review_log_path(review_name)
            .with_context(|| format!("failed to resolve log path for review {review_name}"))
    }

    /// # Errors
    ///
    /// Returns an error when the review cannot be loaded, the clock is invalid, the state
    /// transition is rejected, or the updated review cannot be saved.
    pub async fn set_state(&self, name: &str, next: ReviewState) -> Result<ReviewSession> {
        let mut session = self.load_review(name).await?;
        session
            .set_state(next, now_ms()?)
            .map_err(|error| anyhow!(error))?;
        self.store
            .save_review(&session)
            .await
            .context("failed to save state change")?;
        Ok(session)
    }

    /// # Errors
    ///
    /// Returns an error when the review cannot be loaded, the clock is invalid, or the updated
    /// review cannot be saved.
    pub async fn set_state_force(&self, name: &str, next: ReviewState) -> Result<ReviewSession> {
        let mut session = self.load_review(name).await?;
        session
            .set_state_force(next, now_ms()?)
            .map_err(|error| anyhow!(error))?;
        self.store
            .save_review(&session)
            .await
            .context("failed to save forced state change")?;
        Ok(session)
    }

    /// # Errors
    ///
    /// Returns an error when the review cannot be loaded, the clock is invalid, or the new comment
    /// cannot be persisted.
    pub async fn add_comment(&self, name: &str, input: AddCommentInput) -> Result<ReviewSession> {
        let mut session = self.load_review(name).await?;
        session.add_comment(
            NewLineComment {
                file_path: input.file_path,
                old_line: input.old_line,
                new_line: input.new_line,
                side: input.side,
                line_anchor: input.line_anchor,
                body: input.body,
                author: input.author,
            },
            now_ms()?,
        );
        self.store
            .save_review(&session)
            .await
            .context("failed to persist new comment")?;
        Ok(session)
    }

    /// # Errors
    ///
    /// Returns an error when the review cannot be loaded, the target comment is missing, the clock
    /// is invalid, or the reply cannot be persisted.
    pub async fn add_reply(&self, name: &str, input: AddReplyInput) -> Result<ReviewSession> {
        let mut session = self.load_review(name).await?;
        session
            .add_reply(input.comment_id, input.author, input.body, now_ms()?)
            .map_err(|error| anyhow!(error))?;
        self.store
            .save_review(&session)
            .await
            .context("failed to persist new reply")?;
        Ok(session)
    }

    /// # Errors
    ///
    /// Returns an error when the review cannot be loaded, the actor may not mark the comment
    /// addressed, the clock is invalid, or the update cannot be persisted.
    pub async fn mark_addressed(
        &self,
        name: &str,
        comment_id: u64,
        actor: Author,
    ) -> Result<ReviewSession> {
        self.set_comment_status(name, comment_id, CommentStatus::Addressed, actor)
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the review cannot be loaded, the actor may not reopen the comment, the
    /// clock is invalid, or the update cannot be persisted.
    pub async fn mark_open(
        &self,
        name: &str,
        comment_id: u64,
        actor: Author,
    ) -> Result<ReviewSession> {
        self.set_comment_status(name, comment_id, CommentStatus::Open, actor)
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the review cannot be loaded, the target comment is missing, the clock
    /// is invalid, or the update cannot be persisted.
    pub async fn force_mark_addressed(&self, name: &str, comment_id: u64) -> Result<ReviewSession> {
        let mut session = self.load_review(name).await?;
        session
            .set_comment_status_force(comment_id, CommentStatus::Addressed, now_ms()?)
            .map_err(|error| anyhow!(error))?;
        self.store
            .save_review(&session)
            .await
            .context("failed to persist forced comment status")?;
        Ok(session)
    }

    /// # Errors
    ///
    /// Returns an error when the review cannot be loaded, the target comment is missing, the clock
    /// is invalid, or the re-anchor cannot be persisted.
    pub async fn reanchor_comment(
        &self,
        name: &str,
        input: ReanchorCommentInput,
    ) -> Result<ReviewSession> {
        let mut session = self.load_review(name).await?;
        session
            .reanchor_comment(
                input.comment_id,
                ReanchorLineComment {
                    file_path: input.file_path,
                    old_line: input.old_line,
                    new_line: input.new_line,
                    side: input.side,
                    line_anchor: input.line_anchor,
                },
                now_ms()?,
            )
            .map_err(|error| anyhow!(error))?;
        self.store
            .save_review(&session)
            .await
            .context("failed to persist comment re-anchor")?;
        Ok(session)
    }

    /// # Errors
    ///
    /// Returns an error when the review session cannot be saved.
    pub async fn save_review(&self, session: &ReviewSession) -> Result<()> {
        self.store
            .save_review(session)
            .await
            .context("failed to save review session")
    }

    async fn set_comment_status(
        &self,
        name: &str,
        comment_id: u64,
        status: CommentStatus,
        actor: Author,
    ) -> Result<ReviewSession> {
        let mut session = self.load_review(name).await?;
        session
            .set_comment_status(comment_id, status, actor, now_ms()?)
            .map_err(|error| anyhow!(error))?;
        self.store
            .save_review(&session)
            .await
            .context("failed to persist comment status")?;
        Ok(session)
    }
}
