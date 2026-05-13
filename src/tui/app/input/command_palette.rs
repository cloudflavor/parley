use super::*;

impl TuiApp {
    pub(super) fn open_command_palette(&mut self) {
        self.dismiss_ai_progress_popup();
        self.command_palette = Some(CommandPaletteState {
            query: String::new(),
            cursor_col: 0,
            selected_index: 0,
            scroll: 0,
        });
        self.status_line = "command palette opened".into();
    }

    pub(in crate::tui::app) fn command_palette_items() -> Vec<CommandPaletteItem> {
        vec![
            CommandPaletteItem {
                action: CommandPaletteAction::RefreshReviewAndDiff,
                label: "Refresh Review + Diff",
                keywords: "reload sync",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleFullscreen,
                label: "Toggle Content Fullscreen",
                keywords: "layout zoom",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleSplitDiff,
                label: "Toggle Split View",
                keywords: "layout pane",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleSideBySideDiff,
                label: "Toggle Side-by-Side Diff",
                keywords: "layout unified",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleThreadNavigator,
                label: "Toggle Thread Navigator",
                keywords: "thread sidebar",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::JumpNextThread,
                label: "Jump to Next Thread",
                keywords: "thread next",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::JumpPrevThread,
                label: "Jump to Previous Thread",
                keywords: "thread prev previous",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleSelectedThreadExpansion,
                label: "Toggle Selected Thread Expanded",
                keywords: "thread expand collapse selected",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::SetReviewOpen,
                label: "Set Review State: Open",
                keywords: "state workflow",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::SetReviewUnderReview,
                label: "Set Review State: Under Review",
                keywords: "state workflow",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CycleFileFilter,
                label: "Cycle File Filter",
                keywords: "files open pending",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CycleFileSort,
                label: "Cycle File Sort",
                keywords: "files order ranking",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleActiveFileGroup,
                label: "Toggle Active File Group",
                keywords: "files group collapse expand",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CollapseAllFileGroups,
                label: "Collapse All File Groups",
                keywords: "files group collapse all",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ShowFileHeatmap,
                label: "Show Git File Heatmap",
                keywords: "git history hotspots churn touched files heatmap",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleRootDocumentRendering,
                label: "Toggle Root JSON/Markdown Rendering",
                keywords: "root json markdown pretty prettify render rendering view raw source",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::OpenUserNameEditor,
                label: "Edit User Name",
                keywords: "settings profile",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::OpenThemePicker,
                label: "Open Theme Picker",
                keywords: "theme colors",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::OpenCommitPicker,
                label: "Open Commit Picker",
                keywords: "git commit sha revision ref",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::OpenReviewPicker,
                label: "Open Review Picker",
                keywords: "review context session comments threads",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::OpenThreadSelector,
                label: "Open Thread Selector",
                keywords: "thread comments jump selector navigate",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::OpenCodeSearch,
                label: "Search Codebase",
                keywords: "search code rg grep find files text",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CreateReview,
                label: "Create Review",
                keywords: "review new create context session",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleLightDarkTheme,
                label: "Toggle Theme Light/Dark Variant",
                keywords: "theme light dark",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CycleAiProvider,
                label: "Cycle AI Provider",
                keywords: "ai model provider",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleAiTransport,
                label: "Toggle AI Transport",
                keywords: "ai transport acp cli provider",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::RunAiReviewRefactor,
                label: "Run AI Refactor on Review",
                keywords: "ai run review refactor",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::RunAiThreadRefactor,
                label: "Run AI Refactor on Selected Thread",
                keywords: "ai run thread refactor",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::RunAiThreadReply,
                label: "Run AI Reply on Selected Thread",
                keywords: "ai run thread reply",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CancelAiRun,
                label: "Cancel AI Run",
                keywords: "ai stop cancel",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ShowAiActivity,
                label: "Show AI Activity",
                keywords: "ai logs activity sessions providers",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::OpenShortcuts,
                label: "Open Help Docs",
                keywords: "help docs keys shortcuts keybindings",
            },
        ]
    }

    pub(in crate::tui::app) fn command_palette_filtered_items(
        query: &str,
        items: &[CommandPaletteItem],
    ) -> Vec<CommandPaletteItem> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return items.to_vec();
        }
        let needle = trimmed.to_lowercase();
        items
            .iter()
            .filter(|item| {
                item.label.to_lowercase().contains(&needle)
                    || item.keywords.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    }

    pub(super) async fn handle_command_palette_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            self.command_palette = None;
            self.status_line = "command palette closed".into();
            return Ok(());
        }

        let all_items = Self::command_palette_items();
        let filtered_items = if let Some(palette) = self.command_palette.as_ref() {
            Self::command_palette_filtered_items(&palette.query, &all_items)
        } else {
            Vec::new()
        };

        if matches!(key.code, KeyCode::Enter) {
            let selected_action = self
                .command_palette
                .as_ref()
                .and_then(|palette| filtered_items.get(palette.selected_index))
                .map(|item| item.action);
            if let Some(action) = selected_action {
                self.command_palette = None;
                if let Err(error) = self.apply_command_palette_action(action, service).await {
                    self.status_line = format!("command failed: {error}");
                }
            }
            return Ok(());
        }

        let Some(palette) = self.command_palette.as_mut() else {
            return Ok(());
        };
        match key.code {
            KeyCode::Up => {
                palette.selected_index = palette.selected_index.saturating_sub(1);
            }
            KeyCode::Down => {
                let max_index = filtered_items.len().saturating_sub(1);
                palette.selected_index = (palette.selected_index + 1).min(max_index);
            }
            KeyCode::Home => {
                palette.selected_index = 0;
            }
            KeyCode::End => {
                palette.selected_index = filtered_items.len().saturating_sub(1);
            }
            KeyCode::PageUp => {
                palette.selected_index = palette.selected_index.saturating_sub(8);
            }
            KeyCode::PageDown => {
                let max_index = filtered_items.len().saturating_sub(1);
                palette.selected_index = (palette.selected_index + 8).min(max_index);
            }
            KeyCode::Left => {
                palette.cursor_col = palette.cursor_col.saturating_sub(1);
            }
            KeyCode::Right => {
                palette.cursor_col = (palette.cursor_col + 1).min(palette.query.chars().count());
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                palette.cursor_col = 0;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                palette.cursor_col = palette.query.chars().count();
            }
            KeyCode::Backspace if palette.cursor_col > 0 => {
                remove_char_at(&mut palette.query, palette.cursor_col - 1);
                palette.cursor_col -= 1;
                palette.selected_index = 0;
                palette.scroll = 0;
            }
            KeyCode::Delete if palette.cursor_col < palette.query.chars().count() => {
                remove_char_at(&mut palette.query, palette.cursor_col);
                palette.selected_index = 0;
                palette.scroll = 0;
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at(&mut palette.query, palette.cursor_col, ch);
                palette.cursor_col += 1;
                palette.selected_index = 0;
                palette.scroll = 0;
            }
            _ => {}
        }

        let refreshed_items = Self::command_palette_filtered_items(&palette.query, &all_items);
        if refreshed_items.is_empty() {
            palette.selected_index = 0;
            palette.scroll = 0;
        } else {
            palette.selected_index = palette
                .selected_index
                .min(refreshed_items.len().saturating_sub(1));
            if palette.selected_index < palette.scroll {
                palette.scroll = palette.selected_index;
            }
            let lower_bound = palette.scroll.saturating_add(8);
            if palette.selected_index > lower_bound {
                palette.scroll = palette.selected_index.saturating_sub(8);
            }
        }
        Ok(())
    }
}
