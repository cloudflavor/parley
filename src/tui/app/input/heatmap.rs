use super::*;

impl TuiApp {
    pub(super) fn handle_file_heatmap_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('M') => {
                self.close_file_heatmap();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_file_heatmap(-1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_file_heatmap(1);
            }
            KeyCode::PageUp => {
                self.scroll_file_heatmap(-10);
            }
            KeyCode::PageDown => {
                self.scroll_file_heatmap(10);
            }
            KeyCode::Char('s') => {
                self.cycle_file_heatmap_sort();
            }
            KeyCode::Char('S') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.toggle_file_heatmap_sort_direction();
            }
            KeyCode::Home | KeyCode::Char('g') => {
                if let Some(heatmap) = self.file_heatmap.as_mut() {
                    heatmap.scroll = 0;
                }
            }
            KeyCode::End => {
                if let Some(heatmap) = self.file_heatmap.as_mut() {
                    heatmap.scroll = usize::MAX;
                }
            }
            KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                if let Some(heatmap) = self.file_heatmap.as_mut() {
                    heatmap.scroll = usize::MAX;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
