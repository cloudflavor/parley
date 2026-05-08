use super::*;

impl TuiApp {
    pub(super) async fn apply_command_palette_action(
        &mut self,
        action: CommandPaletteAction,
        service: &ReviewService,
    ) -> Result<()> {
        match action {
            CommandPaletteAction::ToggleFullscreen => {
                self.toggle_content_fullscreen();
            }
            CommandPaletteAction::ToggleSplitDiff => {
                self.toggle_split_diff_view();
                self.status_line = if self.split_diff_view {
                    "split diff enabled".into()
                } else {
                    "split diff disabled".into()
                };
            }
            CommandPaletteAction::ToggleSideBySideDiff => {
                self.side_by_side_diff = !self.side_by_side_diff;
                self.config.diff_view = if self.side_by_side_diff {
                    DiffViewMode::SideBySide
                } else {
                    DiffViewMode::Unified
                };
                service.save_config(&self.config).await?;
                self.clear_diff_render_cache();
                self.status_line = if self.side_by_side_diff {
                    "side-by-side diff enabled".into()
                } else {
                    "unified diff enabled".into()
                };
            }
            CommandPaletteAction::ToggleThreadNavigator => {
                self.thread_nav_visible = !self.thread_nav_visible;
                self.status_line = if self.thread_nav_visible {
                    "thread navigator visible".into()
                } else {
                    "thread navigator hidden".into()
                };
            }
            CommandPaletteAction::RefreshReviewAndDiff => {
                self.refresh_review_and_diff(service).await?;
                self.status_line = "refreshed review and diff".into();
            }
            CommandPaletteAction::SetReviewOpen => {
                self.set_state(service, ReviewState::Open).await?;
            }
            CommandPaletteAction::SetReviewUnderReview => {
                self.set_state(service, ReviewState::UnderReview).await?;
            }
            CommandPaletteAction::SetReviewDone => {
                let unresolved_ids = self.unresolved_thread_ids();
                if !unresolved_ids.is_empty() {
                    self.status_line = format!(
                        "done blocked: unresolved threads {}",
                        format_unresolved_ids(&unresolved_ids)
                    );
                    return Ok(());
                }
                self.set_state(service, ReviewState::Done).await?;
            }
            CommandPaletteAction::OpenUserNameEditor => {
                self.open_user_name_editor();
            }
            CommandPaletteAction::OpenThemePicker => {
                self.open_theme_picker();
            }
            CommandPaletteAction::OpenCommitPicker => {
                self.open_commit_picker()?;
            }
            CommandPaletteAction::OpenReviewPicker => {
                self.open_review_picker(service).await?;
            }
            CommandPaletteAction::CreateReview => {
                self.open_create_review_editor();
            }
            CommandPaletteAction::ToggleLightDarkTheme => {
                self.toggle_light_dark_theme(service).await?;
            }
            CommandPaletteAction::CycleAiProvider => {
                self.cycle_ai_provider(service).await?;
            }
            CommandPaletteAction::RunAiReviewRefactor => {
                self.start_ai_session(service, false, AiSessionMode::Refactor)
                    .await?;
            }
            CommandPaletteAction::RunAiThreadRefactor => {
                self.start_ai_session(service, true, AiSessionMode::Refactor)
                    .await?;
            }
            CommandPaletteAction::RunAiThreadReply => {
                self.start_ai_session(service, true, AiSessionMode::Reply)
                    .await?;
            }
            CommandPaletteAction::CancelAiRun => {
                self.cancel_ai_task();
            }
            CommandPaletteAction::JumpNextThread => {
                self.ensure_row_cache();
                self.jump_thread(true);
            }
            CommandPaletteAction::JumpPrevThread => {
                self.ensure_row_cache();
                self.jump_thread(false);
            }
            CommandPaletteAction::CycleFileFilter => {
                self.cycle_file_filter_mode();
            }
            CommandPaletteAction::CycleFileSort => {
                self.cycle_file_sort_mode();
            }
            CommandPaletteAction::ToggleActiveFileGroup => {
                self.toggle_active_file_group_collapsed();
            }
            CommandPaletteAction::CollapseAllFileGroups => {
                self.collapse_all_visible_file_groups();
            }
            CommandPaletteAction::CycleThreadDensityMode => {
                self.cycle_thread_density_mode();
            }
            CommandPaletteAction::ToggleSelectedThreadExpansion => {
                self.toggle_selected_thread_expansion();
            }
            CommandPaletteAction::OpenShortcuts => {
                self.open_help_docs();
            }
        }
        self.constrain_selection();
        Ok(())
    }
}
