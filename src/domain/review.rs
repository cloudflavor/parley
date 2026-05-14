use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Open,
    UnderReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Author {
    User,
    Ai,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommentStatus {
    Open,
    Pending,
    Addressed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ReviewMutationError {
    #[error("comment_id {comment_id} not found")]
    CommentNotFound { comment_id: u64 },
    #[error("only the original commenter can mark a comment addressed")]
    OnlyOriginalCommenterCanAddress,
    #[error("only the original commenter can change thread status")]
    OnlyOriginalCommenterCanChangeStatus,
}

macro_rules! impl_string_enum {
    ($name:ty, $($variant:ident => $value:literal),+ $(,)?) => {
        impl $name {
            #[must_use]
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $value,)+
                }
            }
        }

        impl FromStr for $name {
            type Err = ();

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(()),
                }
            }
        }
    };
}

impl_string_enum!(ReviewState, Open => "open", UnderReview => "under_review");
impl_string_enum!(Author, User => "user", Ai => "ai");
impl_string_enum!(
    CommentStatus,
    Open => "open",
    Pending => "pending_human",
    Addressed => "addressed",
);
impl_string_enum!(DiffSide, Left => "left", Right => "right");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommentReply {
    pub id: u64,
    pub author: Author,
    pub body: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LineAnchorSnapshot {
    pub target_code: String,
    #[serde(default)]
    pub before_context: Vec<String>,
    #[serde(default)]
    pub after_context: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommentLineRange {
    pub start_old_line: Option<u32>,
    pub start_new_line: Option<u32>,
    pub end_old_line: Option<u32>,
    pub end_new_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffAnchorSnapshot {
    pub hunk_header: String,
    #[serde(default)]
    pub hunk_lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SourceAnchorSnapshot {
    pub file_content_hash: Option<String>,
    pub selected_text_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredAnchorSnapshot {
    pub file_path: String,
    pub side: DiffSide,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    #[serde(default)]
    pub line_range: Option<CommentLineRange>,
    #[serde(default)]
    pub selected_text: String,
    #[serde(default)]
    pub before_context: Vec<String>,
    #[serde(default)]
    pub after_context: Vec<String>,
    #[serde(default)]
    pub diff: Option<DiffAnchorSnapshot>,
    #[serde(default)]
    pub source: Option<SourceAnchorSnapshot>,
    #[serde(default)]
    pub base_rev: Option<String>,
    #[serde(default)]
    pub head_rev: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineComment {
    pub id: u64,
    pub file_path: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    #[serde(default)]
    pub line_range: Option<CommentLineRange>,
    pub side: DiffSide,
    #[serde(default)]
    pub line_anchor: Option<LineAnchorSnapshot>,
    #[serde(default)]
    pub original_anchor: Option<StoredAnchorSnapshot>,
    #[serde(default)]
    pub detached: bool,
    pub body: String,
    pub author: Author,
    pub status: CommentStatus,
    pub replies: Vec<CommentReply>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub addressed_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewSession {
    pub name: String,
    pub state: ReviewState,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub comments: Vec<LineComment>,
    pub next_comment_id: u64,
    pub next_reply_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewLineComment {
    pub file_path: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub line_range: Option<CommentLineRange>,
    pub side: DiffSide,
    pub line_anchor: Option<LineAnchorSnapshot>,
    #[serde(default)]
    pub original_anchor: Option<StoredAnchorSnapshot>,
    pub body: String,
    pub author: Author,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReanchorLineComment {
    pub file_path: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub line_range: Option<CommentLineRange>,
    pub side: DiffSide,
    pub line_anchor: Option<LineAnchorSnapshot>,
}

impl ReviewSession {
    #[must_use]
    pub fn new(name: String, now_ms: u64) -> Self {
        Self {
            name,
            state: ReviewState::Open,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            comments: Vec::new(),
            next_comment_id: 1,
            next_reply_id: 1,
        }
    }

    /// # Errors
    ///
    /// Currently does not fail, but returns `Result` to match other state-transition APIs.
    pub fn set_state(&mut self, next: ReviewState, now_ms: u64) -> Result<(), ReviewMutationError> {
        self.state = next;
        self.updated_at_ms = now_ms;
        Ok(())
    }

    pub fn add_comment(&mut self, new_comment: NewLineComment, now_ms: u64) -> u64 {
        let id = self.next_comment_id;
        self.next_comment_id += 1;

        let comment = LineComment {
            id,
            file_path: new_comment.file_path,
            old_line: new_comment.old_line,
            new_line: new_comment.new_line,
            line_range: new_comment.line_range,
            side: new_comment.side,
            line_anchor: new_comment.line_anchor,
            original_anchor: new_comment.original_anchor,
            detached: false,
            body: new_comment.body,
            author: new_comment.author,
            status: CommentStatus::Open,
            replies: Vec::new(),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            addressed_at_ms: None,
        };

        self.comments.push(comment);
        self.reconcile_review_state_from_threads();
        self.updated_at_ms = now_ms;
        id
    }

    /// # Errors
    ///
    /// Returns an error when `comment_id` does not identify an existing comment.
    pub fn add_reply(
        &mut self,
        comment_id: u64,
        author: Author,
        body: String,
        now_ms: u64,
    ) -> Result<u64, ReviewMutationError> {
        let id = self.next_reply_id;
        self.next_reply_id += 1;

        let comment = self
            .comments
            .iter_mut()
            .find(|comment| comment.id == comment_id)
            .ok_or(ReviewMutationError::CommentNotFound { comment_id })?;

        comment.replies.push(CommentReply {
            id,
            author: author.clone(),
            body,
            created_at_ms: now_ms,
        });
        comment.updated_at_ms = now_ms;
        if author == comment.author {
            comment.status = CommentStatus::Open;
            comment.addressed_at_ms = None;
        } else {
            comment.status = CommentStatus::Pending;
            comment.addressed_at_ms = None;
        }
        self.reconcile_review_state_from_threads();
        self.updated_at_ms = now_ms;
        Ok(id)
    }

    /// # Errors
    ///
    /// Returns an error when `comment_id` does not identify an existing comment.
    pub fn reanchor_comment(
        &mut self,
        comment_id: u64,
        target: ReanchorLineComment,
        now_ms: u64,
    ) -> Result<(), ReviewMutationError> {
        let comment = self
            .comments
            .iter_mut()
            .find(|comment| comment.id == comment_id)
            .ok_or(ReviewMutationError::CommentNotFound { comment_id })?;

        comment.file_path = target.file_path;
        comment.old_line = target.old_line;
        comment.new_line = target.new_line;
        comment.line_range = target.line_range;
        comment.side = target.side;
        comment.line_anchor = target.line_anchor;
        comment.detached = false;
        comment.updated_at_ms = now_ms;
        self.updated_at_ms = now_ms;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when the comment is missing or the actor may not apply the requested status.
    pub fn set_comment_status(
        &mut self,
        comment_id: u64,
        status: CommentStatus,
        actor: Author,
        now_ms: u64,
    ) -> Result<(), ReviewMutationError> {
        self.set_comment_status_with_actor(comment_id, status, now_ms, Some(actor))
    }

    /// # Errors
    ///
    /// Returns an error when `comment_id` does not identify an existing comment.
    pub fn set_comment_status_force(
        &mut self,
        comment_id: u64,
        status: CommentStatus,
        now_ms: u64,
    ) -> Result<(), ReviewMutationError> {
        self.set_comment_status_with_actor(comment_id, status, now_ms, None)
    }

    fn set_comment_status_with_actor(
        &mut self,
        comment_id: u64,
        status: CommentStatus,
        now_ms: u64,
        actor: Option<Author>,
    ) -> Result<(), ReviewMutationError> {
        let comment = self
            .comments
            .iter_mut()
            .find(|comment| comment.id == comment_id)
            .ok_or(ReviewMutationError::CommentNotFound { comment_id })?;

        if let Some(actor) = actor {
            match status {
                CommentStatus::Addressed => {
                    if comment.author != actor {
                        return Err(ReviewMutationError::OnlyOriginalCommenterCanAddress);
                    }
                }
                CommentStatus::Open | CommentStatus::Pending => {
                    if comment.author != actor {
                        return Err(ReviewMutationError::OnlyOriginalCommenterCanChangeStatus);
                    }
                }
            }
        }

        comment.status = status;
        comment.updated_at_ms = now_ms;
        comment.addressed_at_ms = if matches!(status, CommentStatus::Addressed) {
            Some(now_ms)
        } else {
            None
        };

        self.reconcile_review_state_from_threads();
        self.updated_at_ms = now_ms;
        Ok(())
    }

    fn reconcile_review_state_from_threads(&mut self) {
        let has_open = self
            .comments
            .iter()
            .any(|comment| matches!(comment.status, CommentStatus::Open));
        self.state = if has_open {
            ReviewState::Open
        } else {
            ReviewState::UnderReview
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Author, CommentStatus, DiffSide, NewLineComment, ReviewMutationError, ReviewSession,
        ReviewState, StoredAnchorSnapshot,
    };
    use anyhow::Result;

    fn user_comment(new_line: u32, body: &str) -> NewLineComment {
        NewLineComment {
            file_path: "src/lib.rs".into(),
            old_line: None,
            new_line: Some(new_line),
            line_range: None,
            side: DiffSide::Right,
            line_anchor: None,
            original_anchor: None,
            body: body.into(),
            author: Author::User,
        }
    }

    fn session_with_user_comment() -> (ReviewSession, u64) {
        let mut session = ReviewSession::new("r1".into(), 1);
        let comment_id = session.add_comment(user_comment(1, "needs refactor"), 2);
        (session, comment_id)
    }

    #[test]
    fn add_reply_from_ai_should_set_pending_and_under_review() -> Result<()> {
        let (mut session, comment_id) = session_with_user_comment();

        session.add_reply(comment_id, Author::Ai, "fixed".into(), 3)?;

        assert_eq!(session.comments[0].status, CommentStatus::Pending);
        assert_eq!(session.state, ReviewState::UnderReview);
        Ok(())
    }

    #[test]
    fn domain_string_representations_round_trip() -> Result<()> {
        assert_eq!(ReviewState::Open.as_str(), "open");
        assert_eq!(
            "under_review"
                .parse::<ReviewState>()
                .expect("state should parse"),
            ReviewState::UnderReview
        );
        assert_eq!(Author::Ai.as_str(), "ai");
        assert_eq!(
            "user".parse::<Author>().expect("author should parse"),
            Author::User
        );
        assert_eq!(CommentStatus::Pending.as_str(), "pending_human");
        assert_eq!(
            "addressed"
                .parse::<CommentStatus>()
                .expect("status should parse"),
            CommentStatus::Addressed
        );
        assert_eq!(DiffSide::Right.as_str(), "right");
        assert_eq!(
            "left".parse::<DiffSide>().expect("side should parse"),
            DiffSide::Left
        );
        Ok(())
    }

    #[test]
    fn add_reply_from_original_commenter_should_reopen_thread() -> Result<()> {
        let (mut session, comment_id) = session_with_user_comment();
        session.add_reply(comment_id, Author::Ai, "proposal".into(), 3)?;

        session.add_reply(comment_id, Author::User, "please revise".into(), 4)?;

        assert_eq!(session.comments[0].status, CommentStatus::Open);
        assert_eq!(session.state, ReviewState::Open);
        Ok(())
    }

    #[test]
    fn set_comment_status_should_require_original_commenter() {
        let (mut session, comment_id) = session_with_user_comment();

        let error = session
            .set_comment_status(comment_id, CommentStatus::Addressed, Author::Ai, 3)
            .expect_err("non-original commenter should not address comment");

        assert_eq!(error, ReviewMutationError::OnlyOriginalCommenterCanAddress);
    }

    #[test]
    fn set_comment_status_should_reject_non_author_status_changes() {
        let (mut session, comment_id) = session_with_user_comment();

        let error = session
            .set_comment_status(comment_id, CommentStatus::Pending, Author::Ai, 3)
            .expect_err("non-original commenter should not change thread status");

        assert_eq!(
            error,
            ReviewMutationError::OnlyOriginalCommenterCanChangeStatus
        );
    }

    #[test]
    fn set_comment_status_force_should_bypass_original_commenter_check() -> Result<()> {
        let (mut session, comment_id) = session_with_user_comment();

        session.set_comment_status_force(comment_id, CommentStatus::Addressed, 3)?;
        assert_eq!(session.comments[0].status, CommentStatus::Addressed);
        Ok(())
    }

    #[test]
    fn all_addressed_should_reconcile_to_under_review() -> Result<()> {
        let (mut session, comment_id) = session_with_user_comment();
        session.set_comment_status(comment_id, CommentStatus::Addressed, Author::User, 3)?;

        assert_eq!(session.state, ReviewState::UnderReview);
        Ok(())
    }

    #[test]
    fn missing_comment_returns_typed_mutation_error() {
        let mut session = ReviewSession::new("r1".into(), 1);

        let error = session
            .set_comment_status(7, CommentStatus::Addressed, Author::User, 2)
            .expect_err("missing comment should return a typed error");

        assert_eq!(
            error,
            ReviewMutationError::CommentNotFound { comment_id: 7 }
        );
        assert_eq!(error.to_string(), "comment_id 7 not found");
    }

    #[test]
    fn add_comment_should_store_original_anchor_snapshot() {
        let mut session = ReviewSession::new("r1".into(), 1);
        let original_anchor = StoredAnchorSnapshot {
            file_path: "src/lib.rs".into(),
            side: DiffSide::Right,
            old_line: None,
            new_line: Some(7),
            line_range: None,
            selected_text: "fn main() {}".into(),
            before_context: vec!["mod cli;".into()],
            after_context: vec!["mod tui;".into()],
            diff: None,
            source: None,
            base_rev: Some("base".into()),
            head_rev: Some("head".into()),
        };

        let mut comment = user_comment(7, "anchor");
        comment.original_anchor = Some(original_anchor.clone());
        session.add_comment(comment, 2);

        assert_eq!(session.comments[0].original_anchor, Some(original_anchor));
    }
}
