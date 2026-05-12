use super::*;
use crate::tui::app::HELP_DOCS;

impl TuiApp {
    pub(super) fn handle_shortcuts_modal_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => {
                self.shortcuts_modal_visible = false;
                self.status_line = "help docs closed".into();
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
                self.cycle_help_doc(false);
                if let Some(doc) = HELP_DOCS.get(self.shortcuts_modal_doc_index) {
                    self.status_line = format!("help doc: {}", doc.title);
                }
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                self.cycle_help_doc(true);
                if let Some(doc) = HELP_DOCS.get(self.shortcuts_modal_doc_index) {
                    self.status_line = format!("help doc: {}", doc.title);
                }
            }
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                let digit = ch as usize - '0' as usize;
                if digit > 0 {
                    self.set_help_doc_index(digit - 1);
                    if let Some(doc) = HELP_DOCS.get(self.shortcuts_modal_doc_index) {
                        self.status_line = format!("help doc: {}", doc.title);
                    }
                }
            }
            KeyCode::Char('<') => {
                self.resize_help_modal(-1);
                self.status_line = "help zoom out".into();
            }
            KeyCode::Char('>') => {
                self.resize_help_modal(1);
                self.status_line = "help zoom in".into();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_add(1);
            }
            KeyCode::PageUp => {
                self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_sub(8);
            }
            KeyCode::PageDown => {
                self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_add(8);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.shortcuts_modal_scroll = 0;
            }
            KeyCode::End => {
                self.shortcuts_modal_scroll = usize::MAX;
            }
            KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.shortcuts_modal_scroll = usize::MAX;
            }
            _ => {}
        }
        Ok(())
    }
}
