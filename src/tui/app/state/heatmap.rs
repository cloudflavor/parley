//! Git file heatmap state and background loading.

use super::*;

const FILE_HEATMAP_COMMIT_LIMIT: usize = 1_000;

impl TuiApp {
    pub(crate) fn start_file_heatmap(&mut self) {
        self.dismiss_ai_progress_popup();
        if self.file_heatmap_task.is_some() {
            self.status_line = "file heatmap already loading".into();
            return;
        }
        self.file_heatmap = Some(FileHeatmapState {
            entries: Vec::new(),
            scroll: 0,
            loaded_at: None,
        });
        self.file_heatmap_started_at = Some(Instant::now());
        self.file_heatmap_task = Some(task::spawn_blocking(|| {
            file_heatmap(FILE_HEATMAP_COMMIT_LIMIT)
        }));
        self.status_line = "loading git file heatmap".into();
    }

    pub(crate) async fn poll_file_heatmap(&mut self) -> Result<bool> {
        let Some(task) = self.file_heatmap_task.as_ref() else {
            return Ok(false);
        };
        if !task.is_finished() {
            return Ok(false);
        }

        let task = self
            .file_heatmap_task
            .take()
            .context("file heatmap task missing")?;
        let entries = task.await.context("failed to join file heatmap task")??;
        let count = entries.len();
        self.file_heatmap = Some(FileHeatmapState {
            entries,
            scroll: 0,
            loaded_at: Some(Instant::now()),
        });
        self.file_heatmap_started_at = None;
        self.status_line = format!("loaded git file heatmap for {count} file(s)");
        Ok(true)
    }

    pub(crate) fn close_file_heatmap(&mut self) {
        if let Some(task) = self.file_heatmap_task.take() {
            task.abort();
        }
        self.file_heatmap_started_at = None;
        self.file_heatmap = None;
        self.status_line = "file heatmap closed".into();
    }

    pub(crate) fn scroll_file_heatmap(&mut self, delta: isize) {
        let Some(heatmap) = self.file_heatmap.as_mut() else {
            return;
        };
        if delta.is_negative() {
            heatmap.scroll = heatmap.scroll.saturating_sub(delta.unsigned_abs());
        } else {
            heatmap.scroll = heatmap.scroll.saturating_add(delta as usize);
        }
    }

    pub(crate) fn file_heatmap_is_loading(&self) -> bool {
        self.file_heatmap_task.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::tests::make_test_app;

    #[test]
    fn file_heatmap_is_not_loaded_on_startup() -> Result<()> {
        let app = make_test_app(vec!["src/a.rs"], Vec::new())?;

        assert!(app.file_heatmap.is_none());
        assert!(app.file_heatmap_task.is_none());
        assert!(app.file_heatmap_started_at.is_none());
        Ok(())
    }
}
