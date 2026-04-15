use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};

use crate::{
    domain::{
        config::AppConfig,
        review::{Author, CommentStatus, DiffSide, NewLineComment, ReviewSession, ReviewState},
    },
    persistence::store::Store,
};

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
    pub body: String,
    pub author: Author,
}

#[derive(Debug, Clone)]
pub struct AddReplyInput {
    pub comment_id: u64,
    pub author: Author,
    pub body: String,
}

impl ReviewService {
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    pub async fn create_review(&self, name: &str) -> Result<ReviewSession> {
        let session = ReviewSession::new(name.to_string(), now_ms()?);
        self.store
            .create_review(&session)
            .await
            .with_context(|| format!("failed to create review {name}"))?;
        Ok(session)
    }

    pub async fn load_review(&self, name: &str) -> Result<ReviewSession> {
        self.store
            .load_review(name)
            .await
            .with_context(|| format!("failed to load review {name}"))
    }

    pub async fn load_or_create_review(&self, name: &str) -> Result<ReviewSession> {
        match self.store.load_review(name).await {
            Ok(review) => Ok(review),
            Err(_) => self.create_review(name).await,
        }
    }

    pub async fn list_reviews(&self) -> Result<Vec<String>> {
        self.store
            .list_reviews()
            .await
            .context("failed to list reviews")
    }

    pub async fn load_config(&self) -> Result<AppConfig> {
        self.store
            .load_config()
            .await
            .context("failed to load parler config")
    }

    pub async fn save_config(&self, config: &AppConfig) -> Result<()> {
        self.store
            .save_config(config)
            .await
            .context("failed to save parler config")
    }

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

    pub async fn add_comment(&self, name: &str, input: AddCommentInput) -> Result<ReviewSession> {
        let mut session = self.load_review(name).await?;
        session.add_comment(
            NewLineComment {
                file_path: input.file_path,
                old_line: input.old_line,
                new_line: input.new_line,
                side: input.side,
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

    pub async fn mark_addressed(
        &self,
        name: &str,
        comment_id: u64,
        actor: Author,
    ) -> Result<ReviewSession> {
        self.set_comment_status(name, comment_id, CommentStatus::Addressed, actor)
            .await
    }

    pub async fn mark_open(
        &self,
        name: &str,
        comment_id: u64,
        actor: Author,
    ) -> Result<ReviewSession> {
        self.set_comment_status(name, comment_id, CommentStatus::Open, actor)
            .await
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

fn now_ms() -> Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?;
    Ok(elapsed.as_millis() as u64)
}
