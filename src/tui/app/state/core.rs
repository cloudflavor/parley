use super::*;

impl TuiApp {
    pub(crate) fn new(init: TuiAppInit) -> Self {
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
        let ai_transport = config.ai.default_transport;
        let side_by_side_diff = config.diff_view.is_side_by_side();
        let comment_indices_by_file = Self::build_comment_index(&review);
        let comment_stats_by_file = Self::build_comment_stats(&review);
        Self {
            review_name,
            review,
            comment_indices_by_file,
            comment_stats_by_file,
            diff_source,
            config,
            themes,
            theme_index,
            diff,
            ai_provider,
            ai_transport,
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
            selected_visual_row: None,
            secondary_selected_visual_row: None,
            comment_selection_anchor: None,
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
            thread_selector: None,
            code_search: None,
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
            ai_tasks: Vec::new(),
            ai_progress_visible: false,
            ai_activity_visible: false,
            ai_activity_selected: 0,
            ai_activity_scroll: 0,
            selected_ai_log_session_id: None,
            next_ai_log_session_id: 1,
            ai_log_sessions_by_file: HashMap::new(),
            ai_progress_scroll: 0,
            ai_progress_follow_tail: true,
            file_heatmap: None,
            file_heatmap_task: None,
            file_heatmap_started_at: None,
            root_diff_load_task: None,
            root_file_load_task: None,
            root_hydrated_files: std::collections::HashSet::new(),
            root_diff_load_started_at: None,
            root_document_rendering: false,
            shortcuts_modal_visible: false,
            shortcuts_modal_scroll: 0,
            shortcuts_modal_doc_index: 0,
            shortcuts_modal_zoom_step: 0,
            search_query: None,
            last_ai_progress_area: None,
            last_shortcuts_modal_area: None,
            last_file_heatmap_area: None,
            last_file_area: None,
            last_file_search_area: None,
            last_code_search_area: None,
            last_ai_activity_area: None,
            last_thread_selector_area: None,
            last_thread_selector_scroll: 0,
            last_thread_selector_visible_rows: 0,
            last_code_search_scroll: 0,
            last_code_search_visible_rows: 0,
            last_file_scroll: 0,
            file_sidebar_manual_scroll: false,
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

    pub(crate) fn theme(&self) -> &UiTheme {
        &self.themes[self.theme_index]
    }

    pub(crate) fn author_label(&self, author: &Author) -> &str {
        match author {
            Author::User => &self.config.user_name,
            Author::Ai => "AI",
        }
    }

    pub(crate) fn requires_periodic_redraw(&self) -> bool {
        !self.ai_tasks.is_empty()
            || self.file_heatmap_started_at.is_some()
            || self.root_diff_load_started_at.is_some()
            || self.pending_z_prefix_at.is_some()
            || self
                .status_toast_until
                .is_some_and(|deadline| std::time::Instant::now() < deadline)
    }

    pub(crate) fn refresh_status_toast(&mut self) {
        let now = std::time::Instant::now();
        if self.status_line != self.last_status_line_snapshot {
            self.last_status_line_snapshot = self.status_line.clone();
            if self.status_line.trim().is_empty() || self.status_line == "ready" {
                self.status_toast_message = None;
                self.status_toast_until = None;
            } else {
                self.status_toast_message = Some(self.status_line.clone());
                self.status_toast_until = now.checked_add(std::time::Duration::from_secs(4));
            }
        }

        if self
            .status_toast_until
            .is_some_and(|deadline| now >= deadline)
        {
            self.status_toast_until = None;
            self.status_toast_message = None;
        }
    }

    pub(crate) fn invalidate_redraw(&mut self) {
        self.redraw_invalidated = true;
    }

    pub(crate) fn take_redraw_invalidation(&mut self) -> bool {
        std::mem::replace(&mut self.redraw_invalidated, false)
    }

    pub(crate) fn focus_selected_comment_line(&mut self) {
        self.ensure_row_cache();
        let Some(comment) = self.selected_comment_details() else {
            return;
        };

        let rows = self.current_rows();
        let target_row = rows
            .iter()
            .enumerate()
            .find(|(_, row)| anchor::row_matches_exact_anchor(comment, row))
            .or_else(|| {
                matches!(self.diff_source, DiffSource::RootDirectory).then(|| {
                    rows.iter()
                        .enumerate()
                        .find(|(_, row)| comment_reference_matches_display_row(comment, row))
                })?
            });
        let Some(target_row) = target_row else {
            return;
        };

        self.set_active_line_index(target_row.0);
    }

    pub(crate) fn request_scroll_to_thread_tail(
        &mut self,
        pane: DiffPane,
        source_row_index: usize,
    ) {
        let target = match pane {
            DiffPane::Primary => self
                .last_diff_row_map
                .iter()
                .enumerate()
                .filter_map(|(visual_row, &row_index)| {
                    (row_index == source_row_index).then_some(visual_row)
                })
                .next_back(),
            DiffPane::Secondary => self
                .last_diff_row_map_secondary
                .iter()
                .enumerate()
                .filter_map(|(visual_row, &row_index)| {
                    (row_index == source_row_index).then_some(visual_row)
                })
                .next_back(),
        };

        match pane {
            DiffPane::Primary => {
                self.pending_scroll_anchor_row = target;
            }
            DiffPane::Secondary => {
                self.pending_scroll_anchor_row_secondary = target;
            }
        }
    }
}
