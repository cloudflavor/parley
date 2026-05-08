use super::*;

impl TuiApp {
    pub(super) fn resolve_file_reference_hit(
        &self,
        pane: DiffPane,
        rendered_row_index: usize,
        content_col: usize,
    ) -> Option<(String, Option<u32>)> {
        let hits = if matches!(pane, DiffPane::Primary) {
            &self.last_diff_link_hits
        } else {
            &self.last_diff_link_hits_secondary
        };
        hits.iter()
            .find(|hit| {
                hit.rendered_row_index == rendered_row_index
                    && content_col >= hit.col_start
                    && content_col < hit.col_end
            })
            .map(|hit| (hit.path.clone(), hit.line))
    }

    pub(super) fn follow_file_reference(
        &mut self,
        pane: DiffPane,
        raw_path: &str,
        line: Option<u32>,
    ) {
        self.activate_pane(pane);
        let Some(file_index) = self.resolve_file_reference_index(raw_path) else {
            self.status_line = format!("referenced file not in current diff: {raw_path}");
            return;
        };

        self.select_file(file_index);
        if let Some(target_line) = line {
            if self.goto_line_number(target_line) {
                self.status_line = format!(
                    "jumped to {}:{}",
                    self.diff.files[file_index].path, target_line
                );
            } else {
                self.status_line = format!(
                    "opened {}, line {} not found in visible diff hunk",
                    self.diff.files[file_index].path, target_line
                );
            }
        } else {
            self.status_line = format!("opened {}", self.diff.files[file_index].path);
        }
    }

    pub(super) fn resolve_file_reference_index(&self, raw_path: &str) -> Option<usize> {
        let cleaned = raw_path.trim().trim_start_matches("./").replace('\\', "/");
        if cleaned.is_empty() {
            return None;
        }
        if let Some(index) = self.diff.files.iter().position(|file| file.path == cleaned) {
            return Some(index);
        }

        let slash_cleaned = if cleaned.starts_with('/') {
            cleaned.clone()
        } else {
            format!("/{cleaned}")
        };
        self.diff.files.iter().position(|file| {
            cleaned.ends_with(&file.path) || slash_cleaned.ends_with(&format!("/{}", file.path))
        })
    }
}
