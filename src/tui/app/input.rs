use super::{
    CodeSearchResult, CodeSearchState, CommandPaletteAction, CommandPaletteItem,
    CommandPaletteState, CommandPromptMode, CommentLineRange, CommentTarget, DiffPane, DisplayRow,
    INLINE_FILE_MENTION_MAX_CANDIDATES, INLINE_FILE_MENTION_MAX_VISIBLE_ROWS, InlineCommentState,
    InlineDraftMode, InlineFileMentionState, MOUSE_WHEEL_FILE_SCROLL_FILES,
    MOUSE_WHEEL_SCROLL_LINES, PendingUiAction, ReplyTarget, SettingsEditorKind,
    SettingsEditorState, TextBuffer, ThreadAnchor, TuiApp, apply_single_line_edit_key,
    format_comment_reference, format_line_range_reference, format_line_reference, insert_char_at,
    point_in_rect, remove_char_at,
};
use crate::domain::ai::AiSessionMode;
use crate::domain::config::DiffViewMode;
use crate::domain::diff::DiffLineKind;
use crate::domain::review::{Author, DiffSide, LineComment, ReviewState};
use crate::services::review_service::{
    AddCommentInput, AddReplyInput, ReanchorCommentInput, ReviewService,
};
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::time::{Duration, Instant};

mod code_search;
mod command_actions;
mod command_palette;
mod file_reference;
mod heatmap;
mod help;
mod inline_comment;
mod inline_file_picker;
mod mouse;
mod navigation;
mod normal;
mod pickers;
mod search;
mod threads;

impl TuiApp {
    const Z_PREFIX_TIMEOUT: Duration = Duration::from_millis(275);

    pub(super) fn flush_pending_key_sequences(&mut self) -> bool {
        if let Some(pressed_at) = self.pending_z_prefix_at
            && pressed_at.elapsed() >= Self::Z_PREFIX_TIMEOUT
        {
            self.pending_z_prefix_at = None;
            self.toggle_content_fullscreen();
            return true;
        }
        false
    }

    pub(super) async fn handle_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if is_code_search_shortcut(key) {
            self.open_code_search().await?;
            return Ok(());
        }
        if self.ai_activity_visible && self.handle_ai_activity_key(key)? {
            return Ok(());
        }
        if self.file_heatmap.is_some() || self.file_heatmap_started_at.is_some() {
            return self.handle_file_heatmap_key(key);
        }
        if self.shortcuts_modal_visible {
            return self.handle_shortcuts_modal_key(key);
        }
        if self.command_palette.is_some() {
            return self.handle_command_palette_key(key, service).await;
        }
        if self.thread_selector.is_some() {
            return self.handle_thread_selector_key(key);
        }
        if self.code_search.is_some() {
            return self.handle_code_search_key(key).await;
        }
        if self.theme_picker.is_some() {
            return self.handle_theme_picker_key(key, service).await;
        }
        if self.commit_picker.is_some() {
            return self.handle_commit_picker_key(key, service).await;
        }
        if self.review_picker.is_some() {
            return self.handle_review_picker_key(key, service).await;
        }
        if self.settings_editor.is_some() {
            return self.handle_settings_editor_key(key, service).await;
        }
        if self.inline_comment.is_some() {
            return self.handle_inline_comment_key(key, service).await;
        }
        if self.command_prompt.is_some() {
            return self.handle_command_prompt_key(key).await;
        }
        if self.file_search.focused {
            return self.handle_file_search_key(key);
        }

        self.handle_normal_key(key, service).await
    }
}

fn is_code_search_shortcut(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('g' | 'G')) && key.modifiers.contains(KeyModifiers::CONTROL)
        || matches!(key.code, KeyCode::Char('\u{7}'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::AppConfig;
    use crate::domain::diff::{DiffDocument, DiffFile, DiffHunk, DiffLine, DiffLineKind};
    use crate::domain::review::{
        LineAnchorSnapshot, ReviewSession, ReviewState, StoredAnchorSnapshot,
    };
    use crate::git::diff::DiffSource;
    use crate::persistence::store::Store;
    use crate::tui::app::{InlineFileReferencePickerState, TuiAppInit};
    use crate::tui::theme::load_themes;
    use crate::utils::cast::usize_to_u32_saturating;
    use anyhow::{Result, anyhow};
    use ratatui::layout::Rect;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn opening_command_palette_hides_ai_progress_popup() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        app.ai_progress_visible = true;

        app.open_command_palette();

        assert!(app.command_palette.is_some());
        assert!(!app.ai_progress_visible);
        Ok(())
    }

    #[test]
    fn selecting_file_reference_opens_line_picker_in_diff_viewer() -> Result<()> {
        let mut app = make_test_app_with_files(vec![
            empty_diff_file("src/a.rs"),
            diff_file_with_lines(
                "src/target.rs",
                &[(10, "fn ten() {}"), (11, "fn eleven() {}")],
            ),
        ])?;
        app.inline_comment = Some(InlineCommentState {
            row_index: 0,
            mode: InlineDraftMode::Comment(CommentTarget {
                side: DiffSide::Right,
                old_line: None,
                new_line: Some(1),
                line_range: None,
                file_path: "src/a.rs".into(),
                line_anchor: LineAnchorSnapshot::default(),
                original_anchor: Box::new(test_original_anchor("src/a.rs")),
            }),
            buffer: text_buffer_with_line("@src/target.rs"),
            preview_mode: false,
            file_mention: Some(InlineFileMentionState {
                replace_start_col: 0,
                replace_end_col: "@src/target.rs".chars().count(),
                path_query: "src/target.rs".into(),
                line_suffix: None,
                candidates: vec!["src/target.rs".into()],
                selected_index: 0,
                scroll: 0,
            }),
            file_reference_picker: None,
        });

        assert!(app.begin_inline_file_reference_line_picker());
        assert_eq!(app.active_file_index(), 1);
        assert_eq!(app.current_inline_reference_line_number(), Some(10));
        assert_eq!(
            app.inline_comment
                .as_ref()
                .ok_or_else(|| anyhow!("inline comment should exist"))?
                .buffer
                .to_text(),
            "@src/target.rs"
        );
        assert!(
            app.inline_comment
                .as_ref()
                .and_then(|inline| inline.file_reference_picker.as_ref())
                .is_some()
        );
        Ok(())
    }

    #[test]
    fn accepting_file_reference_line_selection_inserts_line_number() -> Result<()> {
        let mut app = make_test_app_with_files(vec![diff_file_with_lines(
            "src/target.rs",
            &[(10, "fn ten() {}"), (11, "fn eleven() {}")],
        )])?;
        app.inline_comment = Some(InlineCommentState {
            row_index: 0,
            mode: InlineDraftMode::Comment(CommentTarget {
                side: DiffSide::Right,
                old_line: None,
                new_line: Some(10),
                line_range: None,
                file_path: "src/target.rs".into(),
                line_anchor: LineAnchorSnapshot::default(),
                original_anchor: Box::new(test_original_anchor("src/target.rs")),
            }),
            buffer: text_buffer_with_line("@src/target.rs"),
            preview_mode: false,
            file_mention: None,
            file_reference_picker: Some(InlineFileReferencePickerState {
                path: "src/target.rs".into(),
                replace_start_col: 0,
                replace_end_col: "@src/target.rs".chars().count(),
                origin_pane: DiffPane::Primary,
                origin_file_index: 0,
                origin_row_index: 0,
            }),
        });
        app.ensure_row_cache();
        assert!(app.goto_line_number(11));

        assert!(app.accept_inline_file_reference_line_selection());

        let inline = app
            .inline_comment
            .as_ref()
            .ok_or_else(|| anyhow!("inline comment should exist"))?;
        assert_eq!(inline.buffer.to_text(), "@src/target.rs:11");
        assert!(inline.file_reference_picker.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn alt_b_moves_backward_by_word_in_inline_comment_editor() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;
        app.inline_comment = Some(InlineCommentState {
            row_index: 0,
            mode: InlineDraftMode::Comment(CommentTarget {
                side: DiffSide::Right,
                old_line: None,
                new_line: Some(1),
                line_range: None,
                file_path: "src/a.rs".into(),
                line_anchor: LineAnchorSnapshot::default(),
                original_anchor: Box::new(test_original_anchor("src/a.rs")),
            }),
            buffer: text_buffer_with_line("alpha  beta"),
            preview_mode: false,
            file_mention: None,
            file_reference_picker: None,
        });

        app.handle_inline_comment_key(
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
            &service,
        )
        .await?;

        let inline = app
            .inline_comment
            .as_ref()
            .ok_or_else(|| anyhow!("inline comment should exist"))?;
        assert_eq!(inline.buffer.cursor_line, 0);
        assert_eq!(inline.buffer.cursor_col, "alpha  ".chars().count());
        Ok(())
    }

    #[tokio::test]
    async fn command_palette_plain_k_filters_instead_of_navigating() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;
        app.open_command_palette();

        app.handle_command_palette_key(
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
            &service,
        )
        .await?;

        let palette = app
            .command_palette
            .as_ref()
            .ok_or_else(|| anyhow!("command palette should remain open"))?;
        assert_eq!(palette.query, "k");
        assert_eq!(palette.cursor_col, 1);
        Ok(())
    }

    #[tokio::test]
    async fn command_palette_can_open_code_search() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;
        let item =
            TuiApp::command_palette_filtered_items("search", &TuiApp::command_palette_items())
                .into_iter()
                .find(|item| item.action == CommandPaletteAction::OpenCodeSearch)
                .ok_or_else(|| anyhow!("search command should be in command palette"))?;

        app.apply_command_palette_action(item.action, &service)
            .await?;

        assert!(app.code_search.is_some());
        assert_eq!(app.status_line, "code search opened");
        Ok(())
    }

    #[tokio::test]
    async fn command_palette_can_toggle_root_document_rendering() -> Result<()> {
        let mut app = make_test_app(vec!["README.md"])?;
        let service = make_test_service()?;
        app.diff_source = DiffSource::RootDirectory;
        let item =
            TuiApp::command_palette_filtered_items("markdown", &TuiApp::command_palette_items())
                .into_iter()
                .find(|item| item.action == CommandPaletteAction::ToggleRootDocumentRendering)
                .ok_or_else(|| anyhow!("rendering command should be in command palette"))?;

        app.apply_command_palette_action(item.action, &service)
            .await?;

        assert!(app.root_document_rendering);
        assert_eq!(app.status_line, "root JSON/Markdown rendering enabled");
        Ok(())
    }

    #[tokio::test]
    async fn shifted_lowercase_d_toggles_root_document_rendering() -> Result<()> {
        let mut app = make_test_app(vec!["README.md"])?;
        let service = make_test_service()?;
        app.diff_source = DiffSource::RootDirectory;

        app.handle_key(
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::SHIFT),
            &service,
        )
        .await?;

        assert!(app.root_document_rendering);
        assert_eq!(app.status_line, "root JSON/Markdown rendering enabled");
        Ok(())
    }

    #[tokio::test]
    async fn ai_transport_toggle_switches_between_acp_and_cli() -> Result<()> {
        use crate::domain::config::{AgentTransport, ProviderTransport};

        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;

        assert_eq!(app.effective_ai_transport(), ProviderTransport::Acp);

        app.handle_key(
            KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT),
            &service,
        )
        .await?;

        assert_eq!(app.effective_ai_transport(), ProviderTransport::Cli);
        assert_eq!(app.config.ai.default_transport, Some(AgentTransport::Cli));

        app.handle_key(
            KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT),
            &service,
        )
        .await?;

        assert_eq!(app.effective_ai_transport(), ProviderTransport::Acp);
        assert_eq!(app.config.ai.default_transport, Some(AgentTransport::Acp));
        Ok(())
    }

    #[tokio::test]
    async fn clicking_code_search_result_opens_match_line() -> Result<()> {
        let mut app = make_test_app_with_files(vec![
            empty_diff_file("src/a.rs"),
            diff_file_with_lines(
                "src/target.rs",
                &[(10, "fn ten() {}"), (11, "fn eleven() {}")],
            ),
        ])?;
        app.code_search = Some(CodeSearchState {
            query: "eleven".into(),
            cursor_col: "eleven".chars().count(),
            results: vec![CodeSearchResult {
                path: "src/target.rs".into(),
                line: 11,
                column: 4,
                text: "fn eleven() {}".into(),
            }],
            selected_index: 0,
            scroll: 0,
            engine: Some("rg"),
            message: "1 match via rg".into(),
        });
        app.last_code_search_area = Some(Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 10,
        });
        app.last_code_search_scroll = 0;
        app.last_code_search_visible_rows = 5;

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 3,
            modifiers: KeyModifiers::empty(),
        })
        .await?;

        assert!(app.code_search.is_none());
        assert_eq!(app.active_file_index(), 1);
        let active_row = app
            .current_rows()
            .get(app.active_line_index())
            .ok_or_else(|| anyhow!("active row should exist"))?;
        assert_eq!(active_row.new_line, Some(11));
        assert_eq!(app.status_line, "opened src/target.rs:11");
        Ok(())
    }

    #[tokio::test]
    async fn code_search_hydrates_root_placeholder_before_opening_match_line() -> Result<()> {
        let path = "src/tui/app/input/code_search.rs";
        let mut app = make_test_app(vec![path])?;
        app.diff_source = DiffSource::RootDirectory;
        app.code_search = Some(CodeSearchState {
            query: "use".into(),
            cursor_col: 3,
            results: vec![CodeSearchResult {
                path: path.into(),
                line: 1,
                column: 1,
                text: "use std::io::ErrorKind;".into(),
            }],
            selected_index: 0,
            scroll: 0,
            engine: Some("rg"),
            message: "1 match via rg".into(),
        });

        app.open_code_search_result_at_index(0).await?;

        let active_row = app
            .current_rows()
            .get(app.active_line_index())
            .ok_or_else(|| anyhow!("active row should exist"))?;
        assert_eq!(active_row.new_line, Some(1));
        assert!(app.root_hydrated_files.contains(&0));
        Ok(())
    }

    #[tokio::test]
    async fn alt_d_deletes_forward_word_in_inline_comment_editor() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;
        app.inline_comment = Some(InlineCommentState {
            row_index: 0,
            mode: InlineDraftMode::Comment(CommentTarget {
                side: DiffSide::Right,
                old_line: None,
                new_line: Some(1),
                line_range: None,
                file_path: "src/a.rs".into(),
                line_anchor: LineAnchorSnapshot::default(),
                original_anchor: Box::new(test_original_anchor("src/a.rs")),
            }),
            buffer: TextBuffer {
                lines: vec!["alpha".into(), "beta gamma".into()],
                cursor_line: 0,
                cursor_col: "alpha".chars().count(),
            },
            preview_mode: false,
            file_mention: None,
            file_reference_picker: None,
        });

        app.handle_inline_comment_key(
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT),
            &service,
        )
        .await?;

        let inline = app
            .inline_comment
            .as_ref()
            .ok_or_else(|| anyhow!("inline comment should exist"))?;
        assert_eq!(inline.buffer.lines, vec!["alpha gamma"]);
        assert_eq!(inline.buffer.cursor_line, 0);
        assert_eq!(inline.buffer.cursor_col, "alpha".chars().count());
        Ok(())
    }

    #[tokio::test]
    async fn ctrl_z_queues_suspend_action() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;

        app.handle_key(
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
            &service,
        )
        .await?;

        assert!(matches!(
            app.pending_action,
            Some(PendingUiAction::SuspendTuiProcess)
        ));
        assert_eq!(app.status_line, "suspending parley; run `fg` to resume");
        Ok(())
    }

    #[tokio::test]
    async fn ctrl_g_opens_code_search() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;

        app.handle_key(
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
            &service,
        )
        .await?;

        assert!(app.code_search.is_some());
        assert_eq!(app.status_line, "code search opened");
        Ok(())
    }

    #[tokio::test]
    async fn ctrl_g_bel_opens_code_search() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;

        app.handle_key(
            KeyEvent::new(KeyCode::Char('\u{7}'), KeyModifiers::NONE),
            &service,
        )
        .await?;

        assert!(app.code_search.is_some());
        assert_eq!(app.status_line, "code search opened");
        Ok(())
    }

    #[tokio::test]
    async fn slash_opens_current_file_search_prompt_not_code_search() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;

        app.handle_key(
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
            &service,
        )
        .await?;

        assert!(app.code_search.is_none());
        assert!(matches!(
            app.command_prompt.as_ref().map(|prompt| &prompt.mode),
            Some(CommandPromptMode::SearchCurrentFile)
        ));
        assert_eq!(app.status_line, "file search prompt");
        Ok(())
    }

    #[test]
    fn file_search_edits_single_line_with_cursor_keys() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        app.file_search.focused = true;

        app.handle_file_search_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))?;
        app.handle_file_search_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))?;
        app.handle_file_search_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))?;
        app.handle_file_search_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT))?;
        app.handle_file_search_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))?;

        assert_eq!(app.file_search.query, "aX");
        assert_eq!(app.file_search.cursor_col, 2);
        Ok(())
    }

    #[tokio::test]
    async fn command_prompt_edits_single_line_with_cursor_keys() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;
        app.open_command_prompt(CommandPromptMode::GotoLine);

        app.handle_key(
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
            &service,
        )
        .await?;
        app.handle_key(
            KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
            &service,
        )
        .await?;
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), &service)
            .await?;
        app.handle_key(
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
            &service,
        )
        .await?;

        let prompt = app
            .command_prompt
            .as_ref()
            .ok_or_else(|| anyhow!("command prompt should remain open"))?;
        assert_eq!(prompt.value, "123");
        assert_eq!(prompt.cursor_col, 2);
        Ok(())
    }

    #[tokio::test]
    async fn shift_v_starts_line_range_selection_without_toggling_split() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;

        app.handle_key(
            KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT),
            &service,
        )
        .await?;

        assert!(!app.split_diff_view);
        assert_eq!(
            app.comment_selection_row_range_for_pane(DiffPane::Primary),
            Some((0, 0))
        );
        assert_eq!(app.status_line, "line range selection started");
        Ok(())
    }

    #[tokio::test]
    async fn ctrl_v_toggles_split_without_starting_line_range_selection() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"])?;
        let service = make_test_service()?;

        app.handle_key(
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL),
            &service,
        )
        .await?;

        assert!(app.split_diff_view);
        assert_eq!(
            app.comment_selection_row_range_for_pane(DiffPane::Primary),
            None
        );
        assert_eq!(app.status_line, "split view enabled");
        Ok(())
    }

    #[tokio::test]
    async fn creating_comment_from_line_range_places_box_at_range_end_and_persists_range()
    -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        service.create_review("test-review").await?;
        let review = service.load_review("test-review").await?;
        let mut app = make_test_app_with_review_and_files(
            review,
            vec![diff_file_with_lines(
                "src/a.rs",
                &[(10, "fn ten() {}"), (11, "fn eleven() {}")],
            )],
        )?;
        app.ensure_row_cache();
        app.set_active_line_index(1);

        app.handle_key(
            KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT),
            &service,
        )
        .await?;
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &service)
            .await?;
        app.handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            &service,
        )
        .await?;

        let inline = app
            .inline_comment
            .as_mut()
            .ok_or_else(|| anyhow!("inline comment should exist"))?;
        assert_eq!(inline.row_index, 2);
        let InlineDraftMode::Comment(target) = &inline.mode else {
            return Err(anyhow!("draft should be a comment"));
        };
        assert_eq!(target.new_line, Some(10));
        assert_eq!(
            target.line_range,
            Some(CommentLineRange {
                start_old_line: Some(10),
                start_new_line: Some(10),
                end_old_line: Some(11),
                end_new_line: Some(11),
            })
        );
        inline.buffer = text_buffer_with_line("range comment");

        app.handle_inline_comment_key(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            &service,
        )
        .await?;

        let updated = service.load_review("test-review").await?;
        let comment = updated
            .comments
            .first()
            .ok_or_else(|| anyhow!("saved comment should exist"))?;
        assert_eq!(
            comment.line_range,
            Some(CommentLineRange {
                start_old_line: Some(10),
                start_new_line: Some(10),
                end_old_line: Some(11),
                end_new_line: Some(11),
            })
        );
        let original_anchor = comment
            .original_anchor
            .as_ref()
            .ok_or_else(|| anyhow!("original anchor should exist"))?;
        assert_eq!(original_anchor.selected_text, "fn ten() {}\nfn eleven() {}");
        assert!(
            app.comment_selection_row_range_for_pane(DiffPane::Primary)
                .is_none()
        );
        Ok(())
    }

    #[tokio::test]
    async fn saving_comment_in_diff_mode_persists_original_hunk_snapshot() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        service.create_review("test-review").await?;
        let review = service.load_review("test-review").await?;
        let mut app = make_test_app_with_review_and_files(
            review,
            vec![diff_file_with_lines(
                "src/a.rs",
                &[(1, "fn one() {}"), (2, "fn two() {}"), (3, "fn three() {}")],
            )],
        )?;
        app.diff_source = DiffSource::Range {
            base: "base-ref".into(),
            head: "head-ref".into(),
        };
        app.ensure_row_cache();
        app.set_active_line_index(2);

        app.handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            &service,
        )
        .await?;
        let inline = app
            .inline_comment
            .as_mut()
            .ok_or_else(|| anyhow!("inline comment should exist"))?;
        inline.buffer = text_buffer_with_line("persist anchor");

        app.handle_inline_comment_key(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            &service,
        )
        .await?;

        let updated = service.load_review("test-review").await?;
        let comment = updated
            .comments
            .first()
            .ok_or_else(|| anyhow!("saved comment should exist"))?;
        let original_anchor = comment
            .original_anchor
            .as_ref()
            .ok_or_else(|| anyhow!("original anchor should exist"))?;
        let diff = original_anchor
            .diff
            .as_ref()
            .ok_or_else(|| anyhow!("diff snapshot should exist"))?;

        assert_eq!(original_anchor.selected_text, "fn two() {}");
        assert_eq!(original_anchor.before_context, vec!["fn one() {}"]);
        assert_eq!(original_anchor.after_context, vec!["fn three() {}"]);
        assert_eq!(diff.hunk_header, "@@ -1,1 +1,1 @@");
        assert!(diff.hunk_lines.contains(&" fn two() {}".to_string()));
        assert_eq!(original_anchor.base_rev.as_deref(), Some("base-ref"));
        assert_eq!(original_anchor.head_rev.as_deref(), Some("head-ref"));
        Ok(())
    }

    #[tokio::test]
    async fn saving_comment_in_root_mode_persists_source_hash_snapshot() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        service.create_review("test-review").await?;
        let review = service.load_review("test-review").await?;
        let mut app = make_test_app_with_review_and_files(
            review,
            vec![root_file_with_lines(
                "src/a.rs",
                &[(1, "fn one() {}"), (2, "fn two() {}")],
            )],
        )?;
        app.diff_source = DiffSource::RootDirectory;
        app.ensure_row_cache();
        app.set_active_line_index(2);

        app.handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            &service,
        )
        .await?;
        let inline = app
            .inline_comment
            .as_mut()
            .ok_or_else(|| anyhow!("inline comment should exist"))?;
        inline.buffer = text_buffer_with_line("persist source anchor");

        app.handle_inline_comment_key(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            &service,
        )
        .await?;

        let updated = service.load_review("test-review").await?;
        let comment = updated
            .comments
            .first()
            .ok_or_else(|| anyhow!("saved comment should exist"))?;
        let original_anchor = comment
            .original_anchor
            .as_ref()
            .ok_or_else(|| anyhow!("original anchor should exist"))?;
        let source = original_anchor
            .source
            .as_ref()
            .ok_or_else(|| anyhow!("source snapshot should exist"))?;

        assert_eq!(original_anchor.selected_text, "fn two() {}");
        assert!(original_anchor.diff.is_none());
        assert!(source.file_content_hash.is_some());
        assert!(source.selected_text_hash.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn pressing_u_reanchors_selected_thread_and_persists_review() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        service.create_review("test-review").await?;
        service
            .add_comment(
                "test-review",
                AddCommentInput {
                    file_path: "src/a.rs".into(),
                    old_line: Some(10),
                    new_line: Some(10),
                    line_range: None,
                    side: DiffSide::Right,
                    line_anchor: None,
                    original_anchor: None,
                    body: "anchor me".into(),
                    author: Author::User,
                },
            )
            .await?;
        let review = service.load_review("test-review").await?;

        let mut app = make_test_app_with_review_and_files(
            review,
            vec![diff_file_with_lines(
                "src/a.rs",
                &[(10, "fn old_anchor() {}"), (12, "fn new_anchor() {}")],
            )],
        )?;
        app.ensure_row_cache();
        assert!(app.goto_line_number(12));
        app.selected_comment = 0;

        app.handle_key(
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::empty()),
            &service,
        )
        .await?;

        let updated = service.load_review("test-review").await?;
        let comment = updated
            .comments
            .iter()
            .find(|comment| comment.id == 1)
            .ok_or_else(|| anyhow!("comment should exist"))?;
        assert_eq!(comment.old_line, Some(12));
        assert_eq!(comment.new_line, Some(12));
        assert!(!comment.detached);
        assert!(comment.line_anchor.is_some());
        assert!(app.status_line.contains("re-anchored"));
        Ok(())
    }

    #[tokio::test]
    async fn saving_new_thread_preserves_current_thread_selection() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        service.create_review("test-review").await?;
        service
            .add_comment(
                "test-review",
                AddCommentInput {
                    file_path: "src/a.rs".into(),
                    old_line: Some(1),
                    new_line: Some(1),
                    line_range: None,
                    side: DiffSide::Right,
                    line_anchor: None,
                    original_anchor: None,
                    body: "first".into(),
                    author: Author::User,
                },
            )
            .await?;
        let review = service.load_review("test-review").await?;
        let mut app = make_test_app_with_review_and_files(
            review,
            vec![diff_file_with_lines(
                "src/a.rs",
                &[(1, "fn first() {}"), (2, "fn second() {}")],
            )],
        )?;
        app.selected_comment = 0;
        app.inline_comment = Some(InlineCommentState {
            row_index: 0,
            mode: InlineDraftMode::Comment(CommentTarget {
                side: DiffSide::Right,
                old_line: Some(2),
                new_line: Some(2),
                line_range: None,
                file_path: "src/a.rs".into(),
                line_anchor: LineAnchorSnapshot::default(),
                original_anchor: Box::new(test_original_anchor("src/a.rs")),
            }),
            buffer: text_buffer_with_line("second"),
            preview_mode: false,
            file_mention: None,
            file_reference_picker: None,
        });

        app.handle_inline_comment_key(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            &service,
        )
        .await?;

        assert_eq!(
            app.selected_comment_details().map(|comment| comment.id),
            Some(1)
        );
        Ok(())
    }

    #[tokio::test]
    async fn saving_reply_restores_replied_thread_selection() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        service.create_review("test-review").await?;
        for (line, body) in [(1, "first"), (2, "second")] {
            service
                .add_comment(
                    "test-review",
                    AddCommentInput {
                        file_path: "src/a.rs".into(),
                        old_line: Some(line),
                        new_line: Some(line),
                        line_range: None,
                        side: DiffSide::Right,
                        line_anchor: None,
                        original_anchor: None,
                        body: body.into(),
                        author: Author::User,
                    },
                )
                .await?;
        }
        let review = service.load_review("test-review").await?;
        let mut app = make_test_app_with_review_and_files(
            review,
            vec![diff_file_with_lines(
                "src/a.rs",
                &[(1, "fn first() {}"), (2, "fn second() {}")],
            )],
        )?;
        app.selected_comment = 1;
        app.inline_comment = Some(InlineCommentState {
            row_index: 0,
            mode: InlineDraftMode::Reply {
                comment_id: 1,
                old_line: Some(1),
                new_line: Some(1),
            },
            buffer: text_buffer_with_line("reply to first"),
            preview_mode: false,
            file_mention: None,
            file_reference_picker: None,
        });

        app.handle_inline_comment_key(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            &service,
        )
        .await?;

        assert_eq!(
            app.selected_comment_details().map(|comment| comment.id),
            Some(1)
        );
        let first = app
            .review
            .comments
            .iter()
            .find(|comment| comment.id == 1)
            .ok_or_else(|| anyhow!("first comment should exist"))?;
        assert_eq!(first.replies.len(), 1);
        Ok(())
    }

    fn make_test_app(paths: Vec<&str>) -> Result<TuiApp> {
        make_test_app_with_files(paths.into_iter().map(empty_diff_file).collect())
    }

    fn make_test_app_with_files(files: Vec<DiffFile>) -> Result<TuiApp> {
        let review = ReviewSession {
            name: "test-review".to_string(),
            state: ReviewState::Open,
            created_at_ms: 0,
            updated_at_ms: 0,
            comments: Vec::new(),
            next_comment_id: 1,
            next_reply_id: 1,
        };
        make_test_app_with_review_and_files(review, files)
    }

    fn make_test_app_with_review_and_files(
        review: ReviewSession,
        files: Vec<DiffFile>,
    ) -> Result<TuiApp> {
        let review = ReviewSession {
            name: review.name,
            state: review.state,
            created_at_ms: review.created_at_ms,
            updated_at_ms: review.updated_at_ms,
            comments: review.comments,
            next_comment_id: review.next_comment_id,
            next_reply_id: review.next_reply_id,
        };
        let diff = DiffDocument { files };
        let themes = load_themes()?;
        Ok(TuiApp::new(TuiAppInit {
            review_name: review.name.clone(),
            review,
            diff,
            diff_source: crate::git::diff::DiffSource::WorkingTree,
            config: AppConfig::default(),
            themes,
            theme_index: 0,
            log_path: PathBuf::from("test.log"),
        }))
    }

    fn empty_diff_file(path: &str) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            header_lines: Vec::new(),
            hunks: Vec::new(),
        }
    }

    fn diff_file_with_lines(path: &str, lines: &[(u32, &str)]) -> DiffFile {
        let mut hunk_lines = vec![DiffLine {
            kind: DiffLineKind::HunkHeader,
            old_line: None,
            new_line: None,
            raw: "@@ -1,1 +1,1 @@".into(),
            code: "@@ -1,1 +1,1 @@".into(),
        }];
        hunk_lines.extend(lines.iter().map(|(line, code)| DiffLine {
            kind: DiffLineKind::Context,
            old_line: Some(*line),
            new_line: Some(*line),
            raw: format!(" {code}"),
            code: (*code).to_string(),
        }));
        DiffFile {
            path: path.to_string(),
            header_lines: Vec::new(),
            hunks: vec![DiffHunk {
                old_start: lines.first().map_or(1, |(line, _)| *line),
                old_count: usize_to_u32_saturating(lines.len()),
                new_start: lines.first().map_or(1, |(line, _)| *line),
                new_count: usize_to_u32_saturating(lines.len()),
                header: "@@ -1,1 +1,1 @@".into(),
                lines: hunk_lines,
            }],
        }
    }

    fn root_file_with_lines(path: &str, lines: &[(u32, &str)]) -> DiffFile {
        let mut hunk_lines = vec![DiffLine {
            kind: DiffLineKind::HunkHeader,
            old_line: None,
            new_line: None,
            raw: "@@ -1,1 +1,1 @@".into(),
            code: "@@ -1,1 +1,1 @@".into(),
        }];
        hunk_lines.extend(lines.iter().map(|(line, code)| DiffLine {
            kind: DiffLineKind::Context,
            old_line: None,
            new_line: Some(*line),
            raw: format!(" {code}"),
            code: (*code).to_string(),
        }));
        DiffFile {
            path: path.to_string(),
            header_lines: Vec::new(),
            hunks: vec![DiffHunk {
                old_start: 1,
                old_count: 0,
                new_start: lines.first().map_or(1, |(line, _)| *line),
                new_count: usize_to_u32_saturating(lines.len()),
                header: "@@ -1,1 +1,1 @@".into(),
                lines: hunk_lines,
            }],
        }
    }

    fn text_buffer_with_line(line: &str) -> TextBuffer {
        TextBuffer {
            lines: vec![line.to_string()],
            cursor_line: 0,
            cursor_col: line.chars().count(),
        }
    }

    fn test_original_anchor(file_path: &str) -> StoredAnchorSnapshot {
        StoredAnchorSnapshot {
            file_path: file_path.to_string(),
            side: DiffSide::Right,
            old_line: None,
            new_line: Some(1),
            line_range: None,
            selected_text: String::new(),
            before_context: Vec::new(),
            after_context: Vec::new(),
            diff: None,
            source: None,
            base_rev: None,
            head_rev: None,
        }
    }

    fn make_test_service() -> Result<ReviewService> {
        let tempdir = tempdir()?;
        Ok(ReviewService::new(Store::from_project_root(tempdir.path())))
    }
}
