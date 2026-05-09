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
            sort_mode: FileHeatmapSortMode::Churn,
            sort_descending: true,
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
        let mut heatmap = FileHeatmapState {
            entries,
            scroll: 0,
            sort_mode: FileHeatmapSortMode::Churn,
            sort_descending: true,
            loaded_at: Some(Instant::now()),
        };
        sort_file_heatmap_entries(
            &mut heatmap.entries,
            heatmap.sort_mode,
            heatmap.sort_descending,
        );
        self.file_heatmap = Some(heatmap);
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

    pub(crate) fn cycle_file_heatmap_sort(&mut self) {
        let Some(heatmap) = self.file_heatmap.as_mut() else {
            return;
        };
        heatmap.sort_mode = heatmap.sort_mode.next();
        heatmap.scroll = 0;
        sort_file_heatmap_entries(
            &mut heatmap.entries,
            heatmap.sort_mode,
            heatmap.sort_descending,
        );
        self.status_line = format!(
            "file heatmap sort: {} {}",
            heatmap.sort_mode.label(),
            heatmap.sort_direction_label()
        );
    }

    pub(crate) fn toggle_file_heatmap_sort_direction(&mut self) {
        let Some(heatmap) = self.file_heatmap.as_mut() else {
            return;
        };
        heatmap.sort_descending = !heatmap.sort_descending;
        heatmap.scroll = 0;
        sort_file_heatmap_entries(
            &mut heatmap.entries,
            heatmap.sort_mode,
            heatmap.sort_descending,
        );
        self.status_line = format!(
            "file heatmap sort: {} {}",
            heatmap.sort_mode.label(),
            heatmap.sort_direction_label()
        );
    }
}

impl FileHeatmapState {
    pub(crate) fn sort_direction_label(&self) -> &'static str {
        if self.sort_descending { "desc" } else { "asc" }
    }
}

impl FileHeatmapSortMode {
    fn next(self) -> Self {
        match self {
            Self::Churn => Self::Added,
            Self::Added => Self::Removed,
            Self::Removed => Self::Commits,
            Self::Commits => Self::NetGrowth,
            Self::NetGrowth => Self::NetShrink,
            Self::NetShrink => Self::Volatility,
            Self::Volatility => Self::Path,
            Self::Path => Self::Churn,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Churn => "churn",
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Commits => "commits",
            Self::NetGrowth => "net-growth",
            Self::NetShrink => "net-shrink",
            Self::Volatility => "volatility",
            Self::Path => "path",
        }
    }
}

fn sort_file_heatmap_entries(
    entries: &mut [FileHeatmapEntry],
    mode: FileHeatmapSortMode,
    descending: bool,
) {
    entries.sort_by(|left, right| {
        let order = match mode {
            FileHeatmapSortMode::Churn => left.changes.cmp(&right.changes),
            FileHeatmapSortMode::Added => left.insertions.cmp(&right.insertions),
            FileHeatmapSortMode::Removed => left.deletions.cmp(&right.deletions),
            FileHeatmapSortMode::Commits => left.commits.cmp(&right.commits),
            FileHeatmapSortMode::NetGrowth => net_growth(left).cmp(&net_growth(right)),
            FileHeatmapSortMode::NetShrink => net_shrink(left).cmp(&net_shrink(right)),
            FileHeatmapSortMode::Volatility => volatility(left).cmp(&volatility(right)),
            FileHeatmapSortMode::Path => right.path.cmp(&left.path),
        };
        if descending {
            order.reverse().then_with(|| left.path.cmp(&right.path))
        } else {
            order.then_with(|| left.path.cmp(&right.path))
        }
    });
}

fn net_growth(entry: &FileHeatmapEntry) -> isize {
    entry.insertions as isize - entry.deletions as isize
}

fn net_shrink(entry: &FileHeatmapEntry) -> isize {
    entry.deletions as isize - entry.insertions as isize
}

fn volatility(entry: &FileHeatmapEntry) -> usize {
    entry.changes.saturating_mul(entry.commits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::history::FileHeatmapEntry;
    use crate::tui::app::state::tests::make_test_app;

    #[test]
    fn file_heatmap_is_not_loaded_on_startup() -> Result<()> {
        let app = make_test_app(vec!["src/a.rs"], Vec::new())?;

        assert!(app.file_heatmap.is_none());
        assert!(app.file_heatmap_task.is_none());
        assert!(app.file_heatmap_started_at.is_none());
        Ok(())
    }

    #[test]
    fn sort_file_heatmap_entries_orders_by_selected_metric() {
        let mut entries = vec![
            heatmap_entry("src/a.rs", 2, 10, 1),
            heatmap_entry("src/b.rs", 1, 3, 20),
            heatmap_entry("src/c.rs", 5, 2, 2),
        ];

        sort_file_heatmap_entries(&mut entries, FileHeatmapSortMode::Removed, true);

        assert_eq!(entries[0].path, "src/b.rs");
    }

    #[test]
    fn cycle_file_heatmap_sort_resets_scroll_and_reorders_entries() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"], Vec::new())?;
        app.file_heatmap = Some(FileHeatmapState {
            entries: vec![
                heatmap_entry("src/a.rs", 2, 10, 1),
                heatmap_entry("src/b.rs", 1, 3, 20),
            ],
            scroll: 9,
            sort_mode: FileHeatmapSortMode::Churn,
            sort_descending: true,
            loaded_at: None,
        });

        app.cycle_file_heatmap_sort();

        let heatmap = app.file_heatmap.as_ref().context("missing heatmap")?;
        assert_eq!(heatmap.sort_mode, FileHeatmapSortMode::Added);
        assert_eq!(heatmap.scroll, 0);
        assert_eq!(heatmap.entries[0].path, "src/a.rs");
        Ok(())
    }

    fn heatmap_entry(
        path: &str,
        commits: usize,
        insertions: usize,
        deletions: usize,
    ) -> FileHeatmapEntry {
        FileHeatmapEntry {
            path: path.to_string(),
            commits,
            changes: insertions + deletions,
            insertions,
            deletions,
        }
    }
}
