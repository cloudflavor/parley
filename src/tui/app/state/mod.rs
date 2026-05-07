pub(crate) mod ai_session;
mod anchor;
pub(crate) mod file_navigation;
pub(crate) mod review;
pub(crate) mod settings;
mod text_buffer;
pub(crate) mod thread_management;
pub(crate) mod viewport;

use super::*;

impl TuiApp {
    pub(super) fn new(init: TuiAppInit) -> Self {
        let TuiAppInit {
            review_name,
            review,
            diff,
            diff_source,
            config,
            themes,
            theme_index,
            log_path,
        } = init;
        let ai_provider = config.ai.default_provider;
        let side_by_side_diff = config.diff_view.is_side_by_side();
        Self {
            review_name,
            review,
            diff_source,
            config,
            themes,
            theme_index,
            diff,
            ai_provider,
            log_path,
            selected_file: 0,
            secondary_selected_file: 0,
            active_diff_pane: DiffPane::Primary,
            split_diff_view: false,
            side_by_side_diff,
            file_pane_width_delta: 0,
            content_fullscreen: false,
            thread_nav_visible: false,
            selected_line: 0,
            secondary_selected_line: 0,
            primary_viewport_top_row: 0,
            secondary_viewport_top_row: 0,
            selected_comment: 0,
            status_line: "ready".to_string(),
            last_status_line_snapshot: "ready".to_string(),
            status_toast_message: None,
            status_toast_until: None,
            last_ai_detail: None,
            inline_comment: None,
            command_palette: None,
            theme_picker: None,
            commit_picker: None,
            review_picker: None,
            file_search: FileSearchState {
                query: String::new(),
                cursor_col: 0,
                focused: false,
            },
            file_filter_mode: FileFilterMode::All,
            file_sort_mode: FileSortMode::Path,
            collapsed_file_groups: std::collections::HashSet::new(),
            thread_density_mode: ThreadDensityMode::Compact,
            expanded_threads: std::collections::HashSet::new(),
            collapsed_threads: std::collections::HashSet::new(),
            settings_editor: None,
            command_prompt: None,
            pending_action: None,
            ai_task: None,
            ai_progress_visible: false,
            ai_progress_lines: VecDeque::with_capacity(AI_PROGRESS_MAX_LINES),
            ai_progress_scroll: 0,
            ai_progress_follow_tail: true,
            shortcuts_modal_visible: false,
            shortcuts_modal_scroll: 0,
            shortcuts_modal_doc_index: 0,
            shortcuts_modal_zoom_step: 0,
            search_query: None,
            last_ai_progress_area: None,
            last_shortcuts_modal_area: None,
            last_file_area: None,
            last_file_search_area: None,
            last_file_scroll: 0,
            last_file_row_map: Vec::new(),
            last_file_group_map: Vec::new(),
            last_diff_area: None,
            last_diff_scroll: 0,
            last_diff_row_map: Vec::new(),
            last_diff_link_hits: Vec::new(),
            pending_scroll_anchor_row: None,
            last_diff_area_secondary: None,
            last_diff_scroll_secondary: 0,
            last_diff_row_map_secondary: Vec::new(),
            last_diff_link_hits_secondary: Vec::new(),
            pending_scroll_anchor_row_secondary: None,
            last_thread_nav_area: None,
            last_thread_nav_scroll: 0,
            last_thread_nav_row_map: Vec::new(),
            row_cache: HashMap::new(),
            diff_render_cache: HashMap::new(),
            diff_render_cache_order: VecDeque::new(),
            pending_z_prefix_at: None,
            redraw_invalidated: true,
            should_quit: false,
        }
    }

    pub(super) fn theme(&self) -> &UiTheme {
        &self.themes[self.theme_index]
    }

    pub(super) fn author_label(&self, author: &Author) -> &str {
        match author {
            Author::User => &self.config.user_name,
            Author::Ai => "AI",
        }
    }

    pub(super) fn requires_periodic_redraw(&self) -> bool {
        self.ai_task.is_some()
    }

    pub(super) fn refresh_status_toast(&mut self) {
        if let Some(until) = self.status_toast_until {
            if until <= anchor::now_ms_utc() {
                self.status_toast_message = None;
                self.status_toast_until = None;
            }
        }
    }

    pub(super) fn invalidate_redraw(&mut self) {
        self.redraw_invalidated = true;
    }

    pub(super) fn take_redraw_invalidation(&mut self) -> bool {
        std::mem::replace(&mut self.redraw_invalidated, false)
    }

    fn normalized_ai_stream_message(stream: &str, message: &str) -> Option<String> {
        if message.is_empty() {
            return None;
        }

        let trimmed = message.trim();
        if trimmed.is_empty() {
            return None;
        }

        if stream == "stderr" && trimmed.starts_with('+') {
            return Some(trimmed[1..].trim().to_string());
        }

        Some(trimmed.to_string())
    }

    fn collect_ai_text_fragments(
        value: &serde_json::Value,
        parent_key: Option<&str>,
    ) -> Vec<String> {
        let mut fragments = Vec::new();

        match value {
            serde_json::Value::String(s) => {
                if Self::is_text_field(parent_key) && !s.trim().is_empty() {
                    fragments.push(s.clone());
                }
            }
            serde_json::Value::Object(map) => {
                for (key, val) in map {
                    fragments.extend(Self::collect_ai_text_fragments(val, Some(key.as_str())));
                }
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    fragments.extend(Self::collect_ai_text_fragments(item, parent_key));
                }
            }
            _ => {}
        }

        fragments
    }

    fn is_text_field(parent_key: Option<&str>) -> bool {
        matches!(
            parent_key,
            Some("content" | "text" | "body" | "message" | "reply" | "output" | "input")
        )
    }

    fn extract_ai_activity_tag(value: &serde_json::Value) -> Option<String> {
        if let serde_json::Value::Object(map) = value {
            if let Some(serde_json::Value::String(tag)) = map.get("tag") {
                return Some(tag.clone());
            }
        }
        None
    }

    pub(super) fn focus_selected_comment_line(&mut self) {
        let Some(comment) = self.selected_comment_details() else {
            return;
        };

        let rows = self.current_rows();
        let Some(target_row) = rows
            .iter()
            .enumerate()
            .find(|(_, row)| anchor::row_matches_exact_anchor(comment, row))
        else {
            return;
        };

        self.set_active_line_index(target_row.0);
    }

    pub(super) fn request_scroll_to_thread_tail(
        &mut self,
        comment_id: u64,
        prefer_expansion: bool,
    ) -> bool {
        let Some(comment) = self.review.comments.iter().find(|c| c.id == comment_id) else {
            return false;
        };

        let file_index = self
            .diff
            .files
            .iter()
            .position(|file| file.path == comment.file_path)?;

        if self.active_file_index() != file_index {
            self.select_file(file_index);
        }

        if prefer_expansion && !self.is_thread_expanded(comment_id, Some(comment_id)) {
            self.expanded_threads.insert(comment_id);
            self.collapsed_threads.remove(&comment_id);
            self.clear_diff_render_cache_for_file(file_index);
        }

        let rows = self.current_rows();
        let mut last_match_index = None;
        for (idx, row) in rows.iter().enumerate() {
            if anchor::row_matches_exact_anchor(comment, row) {
                last_match_index = Some(idx);
            }
        }

        if let Some(target_index) = last_match_index {
            self.set_active_line_index(target_index);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diff::{DiffFile, DiffHunk, DiffLine, DiffLineKind};
    use crate::domain::review::{CommentStatus, LineComment};
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn make_test_app(paths: Vec<&str>, comments: Vec<LineComment>) -> TuiApp {
        make_test_app_with_files_and_comments(
            paths
                .iter()
                .map(|p| diff_file_with_context_lines(p, &[]))
                .collect(),
            comments,
        )
    }

    fn make_test_app_with_files_and_comments(
        files: Vec<DiffFile>,
        comments: Vec<LineComment>,
    ) -> TuiApp {
        let review = ReviewSession {
            name: "test".to_string(),
            created_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            updated_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            state: ReviewState::Open,
            comments,
            state_history: Vec::new(),
        };
        let diff = DiffDocument { files };
        let config = AppConfig::default();
        let themes = load_themes().expect("embedded themes should load");
        let theme_index = resolve_theme_index(&themes, default_theme_name()).unwrap_or(0);

        TuiApp::new(TuiAppInit {
            review_name: "test".to_string(),
            review,
            diff,
            diff_source: DiffSource::WorkingTree,
            config,
            themes,
            theme_index,
            log_path: PathBuf::from("/tmp/test.log"),
        })
    }

    fn diff_file_with_context_lines(path: &str, lines: &[(u32, &str)]) -> DiffFile {
        let mut hunk_lines = Vec::new();
        for (line_num, content) in lines {
            hunk_lines.push(DiffLine {
                kind: DiffLineKind::Context,
                old_line: Some(*line_num),
                new_line: Some(*line_num),
                raw: format!(" {content}"),
                code: content.to_string(),
            });
        }

        DiffFile {
            path: path.to_string(),
            old_path: None,
            header_lines: vec![
                format!("diff --git a/{path} b/{path}"),
                format!("--- a/{path}"),
                format!("+++ b/{path}"),
            ],
            hunks: vec![DiffHunk {
                header: "@@ -1,3 +1,3 @@".to_string(),
                old_start: 1,
                old_count: 3,
                new_start: 1,
                new_count: 3,
                lines: hunk_lines,
            }],
        }
    }

    fn make_comment(id: u64, file_path: &str, status: CommentStatus) -> LineComment {
        LineComment {
            id,
            file_path: file_path.to_string(),
            old_line: None,
            new_line: None,
            line_anchor: None,
            body: "test comment".to_string(),
            status,
            author: Author::User,
            created_at: 0,
            updated_at: 0,
            replies: Vec::new(),
        }
    }

    fn make_comment_with_anchor(
        id: u64,
        file_path: &str,
        status: CommentStatus,
        old_line: u32,
        new_line: u32,
    ) -> LineComment {
        LineComment {
            id,
            file_path: file_path.to_string(),
            old_line: Some(old_line),
            new_line: Some(new_line),
            line_anchor: Some(LineAnchorSnapshot {
                target_code: "test".to_string(),
                before_context: vec![],
                after_context: vec![],
            }),
            body: "test comment".to_string(),
            status,
            author: Author::User,
            created_at: 0,
            updated_at: 0,
            replies: Vec::new(),
        }
    }

    fn cache_key(file_index: usize) -> DiffRenderCacheKey {
        DiffRenderCacheKey {
            file_index,
            pane_inner_width: 80,
            side_by_side_diff: false,
            search_query: None,
            thread_density_mode: ThreadDensityMode::Compact,
            selected_line: 0,
            selected_comment_id: None,
            expanded_thread_ids: vec![],
            review_state_code: 0,
            is_active: true,
        }
    }

    fn cache_entry() -> DiffRenderCacheEntry {
        DiffRenderCacheEntry {
            lines: vec![],
            row_map: vec![],
            link_hits: vec![],
        }
    }
}
