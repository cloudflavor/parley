use serde::{Deserialize, Serialize};
use std::str::FromStr;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommentStatus {
    Open,
    Pending,
    Addressed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffSide {
    Left,
    Right,
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
    pub fn set_state(&mut self, next: ReviewState, now_ms: u64) -> Result<(), String> {
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
    ) -> Result<u64, String> {
        let id = self.next_reply_id;
        self.next_reply_id += 1;

        let comment = self
            .comments
            .iter_mut()
            .find(|comment| comment.id == comment_id)
            .ok_or_else(|| format!("comment_id {comment_id} not found"))?;

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
    ) -> Result<(), String> {
        let comment = self
            .comments
            .iter_mut()
            .find(|comment| comment.id == comment_id)
            .ok_or_else(|| format!("comment_id {comment_id} not found"))?;

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
    ) -> Result<(), String> {
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
    ) -> Result<(), String> {
        self.set_comment_status_with_actor(comment_id, status, now_ms, None)
    }

    fn set_comment_status_with_actor(
        &mut self,
        comment_id: u64,
        status: CommentStatus,
        now_ms: u64,
        actor: Option<Author>,
    ) -> Result<(), String> {
        let comment = self
            .comments
            .iter_mut()
            .find(|comment| comment.id == comment_id)
            .ok_or_else(|| format!("comment_id {comment_id} not found"))?;

        if let Some(actor) = actor {
            match status {
                CommentStatus::Addressed => {
                    if comment.author != actor {
                        return Err(
                            "only the original commenter can mark a comment addressed".to_string()
                        );
                    }
                }
                CommentStatus::Open | CommentStatus::Pending => {
                    if comment.author != actor {
                        return Err(
                            "only the original commenter can change thread status".to_string()
                        );
                    }
                }
            }
        }

        comment.status = status.clone();
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
    use super::{Author, CommentStatus, DiffSide, NewLineComment, ReviewSession, ReviewState};
    use anyhow::Result;

    #[test]
    fn add_reply_from_ai_should_set_pending_and_under_review() -> Result<()> {
        let mut session = ReviewSession::new("r1".into(), 1);
        let comment_id = session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                line_range: None,
                side: DiffSide::Right,
                line_anchor: None,
                body: "needs refactor".into(),
                author: Author::User,
            },
            2,
        );

        session
            .add_reply(comment_id, Author::Ai, "fixed".into(), 3)
            .map_err(anyhow::Error::msg)?;

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
        let mut session = ReviewSession::new("r1".into(), 1);
        let comment_id = session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                line_range: None,
                side: DiffSide::Right,
                line_anchor: None,
                body: "needs refactor".into(),
                author: Author::User,
            },
            2,
        );
        session
            .add_reply(comment_id, Author::Ai, "proposal".into(), 3)
            .map_err(anyhow::Error::msg)?;

        session
            .add_reply(comment_id, Author::User, "please revise".into(), 4)
            .map_err(anyhow::Error::msg)?;

        assert_eq!(session.comments[0].status, CommentStatus::Open);
        assert_eq!(session.state, ReviewState::Open);
        Ok(())
    }

    #[test]
    fn set_comment_status_should_require_original_commenter() {
        let mut session = ReviewSession::new("r1".into(), 1);
        let comment_id = session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                line_range: None,
                side: DiffSide::Right,
                line_anchor: None,
                body: "needs refactor".into(),
                author: Author::User,
            },
            2,
        );

        let result =
            session.set_comment_status(comment_id, CommentStatus::Addressed, Author::Ai, 3);
        assert!(result.is_err());
    }

    #[test]
    fn set_comment_status_force_should_bypass_original_commenter_check() -> Result<()> {
        let mut session = ReviewSession::new("r1".into(), 1);
        let comment_id = session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                line_range: None,
                side: DiffSide::Right,
                line_anchor: None,
                body: "needs refactor".into(),
                author: Author::User,
            },
            2,
        );

        session
            .set_comment_status_force(comment_id, CommentStatus::Addressed, 3)
            .map_err(anyhow::Error::msg)?;
        assert_eq!(session.comments[0].status, CommentStatus::Addressed);
        Ok(())
    }

    #[test]
    fn all_addressed_should_reconcile_to_under_review() -> Result<()> {
        let mut session = ReviewSession::new("r1".into(), 1);
        let comment_id = session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                line_range: None,
                side: DiffSide::Right,
                line_anchor: None,
                body: "needs refactor".into(),
                author: Author::User,
            },
            2,
        );
        session
            .set_comment_status(comment_id, CommentStatus::Addressed, Author::User, 3)
            .map_err(anyhow::Error::msg)?;

        assert_eq!(session.state, ReviewState::UnderReview);
        Ok(())
    }
}
