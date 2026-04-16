use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Draft,
    Pending,
    WaitingForResponse,
    Done,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommentReply {
    pub id: u64,
    pub author: Author,
    pub body: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineComment {
    pub id: u64,
    pub file_path: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub side: DiffSide,
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
    pub done_at_ms: Option<u64>,
    pub comments: Vec<LineComment>,
    pub next_comment_id: u64,
    pub next_reply_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewLineComment {
    pub file_path: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub side: DiffSide,
    pub body: String,
    pub author: Author,
}

impl ReviewSession {
    pub fn new(name: String, now_ms: u64) -> Self {
        Self {
            name,
            state: ReviewState::Draft,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            done_at_ms: None,
            comments: Vec::new(),
            next_comment_id: 1,
            next_reply_id: 1,
        }
    }

    pub fn set_state(&mut self, next: ReviewState, now_ms: u64) -> Result<(), String> {
        let allowed = match (&self.state, &next) {
            (ReviewState::Draft, ReviewState::Pending) => true,
            (ReviewState::Pending, ReviewState::WaitingForResponse) => true,
            (ReviewState::WaitingForResponse, ReviewState::Pending) => true,
            (_, ReviewState::Done) => true,
            (ReviewState::Done, ReviewState::WaitingForResponse) => true,
            (current, wanted) if current == wanted => true,
            _ => false,
        };

        if !allowed {
            return Err(format!(
                "invalid state transition from {:?} to {:?}",
                self.state, next
            ));
        }

        if matches!(next, ReviewState::Done) {
            let unresolved_threads = self
                .comments
                .iter()
                .filter(|comment| !matches!(comment.status, CommentStatus::Addressed))
                .count();
            if unresolved_threads > 0 {
                return Err(format!(
                    "cannot set review to done: {unresolved_threads} unresolved thread(s)"
                ));
            }
        }

        if matches!(next, ReviewState::Done) {
            self.done_at_ms = Some(now_ms);
        }
        if matches!(self.state, ReviewState::Done) && !matches!(next, ReviewState::Done) {
            self.done_at_ms = None;
        }
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
            side: new_comment.side,
            body: new_comment.body,
            author: new_comment.author,
            status: CommentStatus::Open,
            replies: Vec::new(),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            addressed_at_ms: None,
        };

        self.comments.push(comment);
        if matches!(self.state, ReviewState::Done) {
            self.state = ReviewState::Pending;
            self.done_at_ms = None;
        }
        if matches!(self.state, ReviewState::WaitingForResponse) {
            self.state = ReviewState::Pending;
        }
        self.updated_at_ms = now_ms;
        id
    }

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
            .ok_or_else(|| format!("comment_id {} not found", comment_id))?;

        comment.replies.push(CommentReply {
            id,
            author: author.clone(),
            body,
            created_at_ms: now_ms,
        });
        comment.updated_at_ms = now_ms;
        if author != comment.author {
            comment.status = CommentStatus::Pending;
            comment.addressed_at_ms = None;
            if matches!(self.state, ReviewState::Done) {
                self.state = ReviewState::Pending;
                self.done_at_ms = None;
            }
        }
        if matches!(author, Author::Ai) && matches!(self.state, ReviewState::Pending) {
            self.state = ReviewState::WaitingForResponse;
        }
        self.updated_at_ms = now_ms;
        Ok(id)
    }

    pub fn set_comment_status(
        &mut self,
        comment_id: u64,
        status: CommentStatus,
        actor: Author,
        now_ms: u64,
    ) -> Result<(), String> {
        let comment = self
            .comments
            .iter_mut()
            .find(|comment| comment.id == comment_id)
            .ok_or_else(|| format!("comment_id {} not found", comment_id))?;

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
                    return Err("only the original commenter can change thread status".to_string());
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

        if matches!(status, CommentStatus::Open | CommentStatus::Pending)
            && matches!(self.state, ReviewState::Done)
        {
            self.state = ReviewState::Pending;
            self.done_at_ms = None;
        }

        if matches!(status, CommentStatus::Addressed) && matches!(self.state, ReviewState::Pending)
        {
            self.state = ReviewState::WaitingForResponse;
        }
        if matches!(status, CommentStatus::Open)
            && matches!(self.state, ReviewState::WaitingForResponse)
        {
            self.state = ReviewState::Pending;
        }

        self.updated_at_ms = now_ms;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Author, CommentStatus, DiffSide, NewLineComment, ReviewSession, ReviewState};

    #[test]
    fn set_state_should_allow_draft_to_pending() {
        let mut session = ReviewSession::new("r1".into(), 1);
        let result = session.set_state(ReviewState::Pending, 2);

        assert!(result.is_ok());
        assert_eq!(session.state, ReviewState::Pending);
    }

    #[test]
    fn set_state_should_reject_pending_to_draft() {
        let mut session = ReviewSession::new("r1".into(), 1);
        session
            .set_state(ReviewState::Pending, 2)
            .expect("state should move to pending");

        let result = session.set_state(ReviewState::Draft, 3);

        assert!(result.is_err());
    }

    #[test]
    fn set_state_should_reject_done_to_pending() {
        let mut session = ReviewSession::new("r1".into(), 1);
        session
            .set_state(ReviewState::Pending, 2)
            .expect("state should move to pending");
        session
            .set_state(ReviewState::Done, 3)
            .expect("state should move to done");

        let result = session.set_state(ReviewState::Pending, 4);

        assert!(result.is_err());
        assert_eq!(session.state, ReviewState::Done);
        assert_eq!(session.done_at_ms, Some(3));
    }

    #[test]
    fn set_state_should_reject_done_with_unresolved_threads() {
        let mut session = ReviewSession::new("r1".into(), 1);
        session
            .set_state(ReviewState::Pending, 2)
            .expect("state should move to pending");
        session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                side: DiffSide::Right,
                body: "needs refactor".into(),
                author: Author::User,
            },
            3,
        );

        let result = session.set_state(ReviewState::Done, 4);

        assert!(result.is_err());
        assert_eq!(session.state, ReviewState::Pending);
        assert_eq!(session.done_at_ms, None);
    }

    #[test]
    fn set_state_should_allow_done_when_all_threads_are_addressed() {
        let mut session = ReviewSession::new("r1".into(), 1);
        session
            .set_state(ReviewState::Pending, 2)
            .expect("state should move to pending");
        let comment_id = session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                side: DiffSide::Right,
                body: "needs refactor".into(),
                author: Author::User,
            },
            3,
        );
        session
            .set_comment_status(comment_id, CommentStatus::Addressed, Author::User, 4)
            .expect("status should update");

        let result = session.set_state(ReviewState::Done, 5);

        assert!(result.is_ok());
        assert_eq!(session.state, ReviewState::Done);
        assert_eq!(session.done_at_ms, Some(5));
    }

    #[test]
    fn add_comment_should_reopen_done_review() {
        let mut session = ReviewSession::new("r1".into(), 1);
        session
            .set_state(ReviewState::Pending, 2)
            .expect("state should move to pending");
        session
            .set_state(ReviewState::Done, 3)
            .expect("state should move to done");

        session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                side: DiffSide::Right,
                body: "new thread".into(),
                author: Author::User,
            },
            4,
        );

        assert_eq!(session.state, ReviewState::Pending);
        assert_eq!(session.done_at_ms, None);
        assert_eq!(session.comments.len(), 1);
    }

    #[test]
    fn add_reply_should_move_pending_to_waiting_for_response_for_ai_author() {
        let mut session = ReviewSession::new("r1".into(), 1);
        session
            .set_state(ReviewState::Pending, 2)
            .expect("state should move to pending");
        let comment_id = session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                side: DiffSide::Right,
                body: "needs refactor".into(),
                author: Author::User,
            },
            3,
        );

        let reply = session.add_reply(comment_id, Author::Ai, "fixed".into(), 4);

        assert!(reply.is_ok());
        assert_eq!(session.comments[0].status, CommentStatus::Pending);
        assert_eq!(session.state, ReviewState::WaitingForResponse);
    }

    #[test]
    fn set_comment_status_should_track_addressed_time() {
        let mut session = ReviewSession::new("r1".into(), 1);
        let comment_id = session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                side: DiffSide::Right,
                body: "needs refactor".into(),
                author: Author::User,
            },
            3,
        );
        session
            .add_reply(comment_id, Author::Ai, "fixed".into(), 4)
            .expect("ai reply should be added");

        session
            .set_comment_status(comment_id, CommentStatus::Addressed, Author::User, 5)
            .expect("status should update");

        assert_eq!(session.comments[0].status, CommentStatus::Addressed);
        assert_eq!(session.comments[0].addressed_at_ms, Some(5));
    }

    #[test]
    fn set_comment_status_should_require_op_to_address() {
        let mut session = ReviewSession::new("r1".into(), 1);
        let comment_id = session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                side: DiffSide::Right,
                body: "needs refactor".into(),
                author: Author::User,
            },
            2,
        );

        let result =
            session.set_comment_status(comment_id, CommentStatus::Addressed, Author::Ai, 3);

        assert!(result.is_err());
        assert_eq!(session.comments[0].status, CommentStatus::Open);
    }

    #[test]
    fn set_comment_status_should_require_op_to_reopen() {
        let mut session = ReviewSession::new("r1".into(), 1);
        let comment_id = session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                side: DiffSide::Right,
                body: "needs refactor".into(),
                author: Author::User,
            },
            2,
        );
        session
            .add_reply(comment_id, Author::Ai, "fixed".into(), 3)
            .expect("reply should be added");
        session
            .set_comment_status(comment_id, CommentStatus::Addressed, Author::User, 4)
            .expect("op can address");

        let result = session.set_comment_status(comment_id, CommentStatus::Open, Author::Ai, 5);

        assert!(result.is_err());
        assert_eq!(session.comments[0].status, CommentStatus::Addressed);
    }

    #[test]
    fn add_reply_should_not_auto_address_comment() {
        let mut session = ReviewSession::new("r1".into(), 1);
        let comment_id = session.add_comment(
            NewLineComment {
                file_path: "src/lib.rs".into(),
                old_line: None,
                new_line: Some(1),
                side: DiffSide::Right,
                body: "needs refactor".into(),
                author: Author::User,
            },
            2,
        );

        session
            .add_reply(comment_id, Author::Ai, "done".into(), 3)
            .expect("reply should be added");

        assert_eq!(session.comments[0].status, CommentStatus::Pending);
        assert_eq!(session.comments[0].addressed_at_ms, None);
    }
}
