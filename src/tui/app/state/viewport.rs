//! Viewport and rendering cache state.
//!
//! Handles scroll positions, row caches, and diff render caches.

use super::*;

use pulldown_cmark::{
    Event as MdEvent, HeadingLevel, Options as MdOptions, Parser as MdParser, Tag as MdTag,
    TagEnd as MdTagEnd,
};

use crate::domain::diff::{DiffFile, DiffLineKind};
use crate::tui::theme::ThemeColors;

impl TuiApp {
    pub(crate) fn active_line_index(&self) -> usize {
        if self.split_diff_view && matches!(self.active_diff_pane, DiffPane::Secondary) {
            self.secondary_selected_line
        } else {
            self.selected_line
        }
    }

    pub(crate) fn set_active_line_index(&mut self, index: usize) {
        if self.split_diff_view && matches!(self.active_diff_pane, DiffPane::Secondary) {
            if self.secondary_selected_line != index {
                self.pending_scroll_anchor_row_secondary = None;
                self.secondary_selected_visual_row = None;
            }
            self.secondary_selected_line = index;
        } else {
            if self.selected_line != index {
                self.pending_scroll_anchor_row = None;
                self.selected_visual_row = None;
            }
            self.selected_line = index;
        }
    }

    pub(crate) fn set_line_for_pane(&mut self, pane: DiffPane, index: usize) {
        match pane {
            DiffPane::Primary => {
                if self.selected_line != index {
                    self.pending_scroll_anchor_row = None;
                    self.selected_visual_row = None;
                }
                self.selected_line = index;
            }
            DiffPane::Secondary => {
                if self.secondary_selected_line != index {
                    self.pending_scroll_anchor_row_secondary = None;
                    self.secondary_selected_visual_row = None;
                }
                self.secondary_selected_line = index;
            }
        }
    }

    pub(crate) fn visual_row_for_pane(&self, pane: DiffPane) -> Option<usize> {
        match pane {
            DiffPane::Primary => self.selected_visual_row,
            DiffPane::Secondary => self.secondary_selected_visual_row,
        }
    }

    pub(crate) fn set_visual_row_for_pane(&mut self, pane: DiffPane, visual_row: Option<usize>) {
        match pane {
            DiffPane::Primary => {
                self.selected_visual_row = visual_row;
            }
            DiffPane::Secondary => {
                self.secondary_selected_visual_row = visual_row;
            }
        }
    }

    pub(crate) fn comment_selection_row_range_for_pane(
        &self,
        pane: DiffPane,
    ) -> Option<(usize, usize)> {
        let (anchor_pane, anchor_row) = self.comment_selection_anchor?;
        if anchor_pane != pane {
            return None;
        }
        let active_row = self.line_for_pane(pane);
        Some(if anchor_row <= active_row {
            (anchor_row, active_row)
        } else {
            (active_row, anchor_row)
        })
    }

    pub(crate) fn clear_comment_line_selection(&mut self) {
        self.comment_selection_anchor = None;
    }

    pub(crate) fn toggle_comment_line_selection(&mut self) {
        let pane = self.active_diff_pane;
        let active_row = self.line_for_pane(pane);
        if self.comment_selection_anchor == Some((pane, active_row)) {
            self.comment_selection_anchor = None;
            self.status_line = "line range selection cleared".into();
            return;
        }
        self.comment_selection_anchor = Some((pane, active_row));
        self.status_line = "line range selection started".into();
    }

    pub(crate) fn extend_comment_line_selection_to(&mut self, pane: DiffPane, row_index: usize) {
        if !matches!(self.comment_selection_anchor, Some((anchor_pane, _)) if anchor_pane == pane) {
            self.comment_selection_anchor = Some((pane, self.line_for_pane(pane)));
        }
        self.set_line_for_pane(pane, row_index);
        self.status_line = "line range selection extended".into();
    }

    pub(crate) fn viewport_top_for_pane(&self, pane: DiffPane) -> usize {
        match pane {
            DiffPane::Primary => self.primary_viewport_top_row,
            DiffPane::Secondary => self.secondary_viewport_top_row,
        }
    }

    pub(crate) fn set_viewport_top_for_pane(&mut self, pane: DiffPane, top_row: usize) {
        match pane {
            DiffPane::Primary => {
                self.primary_viewport_top_row = top_row;
            }
            DiffPane::Secondary => {
                self.secondary_viewport_top_row = top_row;
            }
        }
    }

    pub(crate) fn take_pending_scroll_anchor(&mut self, pane: DiffPane) -> Option<usize> {
        match pane {
            DiffPane::Primary => self.pending_scroll_anchor_row.take(),
            DiffPane::Secondary => self.pending_scroll_anchor_row_secondary.take(),
        }
    }

    pub(crate) fn row_map_for_pane(&self, pane: DiffPane) -> &[usize] {
        match pane {
            DiffPane::Primary => &self.last_diff_row_map,
            DiffPane::Secondary => &self.last_diff_row_map_secondary,
        }
    }

    pub(crate) fn viewport_height_for_pane(&self, pane: DiffPane) -> usize {
        let area = match pane {
            DiffPane::Primary => self.last_diff_area,
            DiffPane::Secondary => self.last_diff_area_secondary,
        };
        area.map_or(1, |rect| usize::from(rect.height.saturating_sub(2)))
            .max(1)
    }

    pub(crate) fn effective_viewport_height_for_pane(&self, pane: DiffPane) -> usize {
        let base = self.viewport_height_for_pane(pane);
        if self.inline_comment.is_none() || pane != self.active_diff_pane {
            return base;
        }

        let area = match pane {
            DiffPane::Primary => self.last_diff_area,
            DiffPane::Secondary => self.last_diff_area_secondary,
        };
        let reserved_rows = area
            .map(inline_comment_editor_reserved_rows)
            .unwrap_or_default();
        base.saturating_sub(reserved_rows).max(1)
    }

    pub(crate) fn current_rows(&self) -> &[DisplayRow] {
        self.row_cache
            .get(&self.active_file_index())
            .map_or(&[], |cached| cached.rows.as_slice())
    }

    pub(crate) fn line_anchor_snapshot_for_row(
        &self,
        row_index: usize,
    ) -> Option<LineAnchorSnapshot> {
        let rows = self.current_rows();
        let row = rows.get(row_index)?;
        if !anchor::is_commentable_row(row) {
            return None;
        }
        Some(anchor::build_line_anchor_snapshot(rows, row_index))
    }

    pub(crate) fn row_count_for_file(&self, file_index: usize) -> Option<usize> {
        self.row_cache
            .get(&file_index)
            .map(|cached| cached.rows.len())
    }

    pub(crate) fn row_for_file(&self, file_index: usize, row_index: usize) -> Option<&DisplayRow> {
        self.row_cache
            .get(&file_index)
            .and_then(|cached| cached.rows.get(row_index))
    }

    pub(crate) fn syntax_painter_for_file(
        &self,
        file_index: usize,
        theme_colors: &ThemeColors,
    ) -> Option<SyntaxPainter> {
        self.diff
            .files
            .get(file_index)
            .map(|file| SyntaxPainter::for_path(&file.path, theme_colors))
    }

    pub(crate) fn highlighted_segments_for_file_row_with_painter(
        &mut self,
        file_index: usize,
        row_index: usize,
        painter: &mut SyntaxPainter,
        theme_colors: &ThemeColors,
    ) -> HighlightParts {
        self.ensure_row_cache_for_file(file_index);
        let Some(cached) = self.row_cache.get_mut(&file_index) else {
            return Vec::new();
        };
        let Some(row) = cached.rows.get(row_index) else {
            return Vec::new();
        };

        let parsed = match row.kind {
            DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context => {
                painter.highlight(&row.code, theme_colors)
            }
            _ => Vec::new(),
        };
        if let Some(parts) = cached
            .highlights
            .get(row_index)
            .and_then(std::option::Option::as_ref)
        {
            return parts.clone();
        }

        if let Some(slot) = cached.highlights.get_mut(row_index) {
            *slot = Some(parsed.clone());
        }
        parsed
    }

    pub(crate) fn constrain_selection(&mut self) {
        let rows_len = self
            .row_cache
            .get(&self.active_file_index())
            .map_or(0, |cached| cached.rows.len());
        if rows_len == 0 {
            self.set_active_line_index(0);
        } else if self.active_line_index() >= rows_len {
            self.set_active_line_index(rows_len - 1);
        }

        let comments_len = self.comments_for_selected_file().len();
        if comments_len == 0 {
            self.selected_comment = 0;
        } else if self.selected_comment >= comments_len {
            self.selected_comment = comments_len - 1;
        }

        if self.selected_file >= self.diff.files.len() {
            self.selected_file = self.diff.files.len().saturating_sub(1);
        }
        if self.secondary_selected_file >= self.diff.files.len() {
            self.secondary_selected_file = self.diff.files.len().saturating_sub(1);
        }
        self.constrain_active_file_to_visible_list();

        if let Some(inline) = self.inline_comment.as_ref()
            && inline.row_index >= rows_len
        {
            self.inline_comment = None;
        }
    }

    pub(crate) fn ensure_row_cache(&mut self) {
        self.ensure_row_cache_for_file(self.active_file_index());
    }

    pub(crate) fn ensure_row_cache_for_file(&mut self, file_index: usize) {
        if self.row_cache.contains_key(&file_index) {
            return;
        }
        self.rebuild_row_cache_for_file(file_index);
    }

    pub(crate) fn rebuild_row_cache_for_file(&mut self, file_index: usize) {
        let Some(file) = self.diff.files.get(file_index) else {
            self.row_cache.remove(&file_index);
            self.clear_diff_render_cache_for_file(file_index);
            return;
        };

        let mut rows = Vec::new();
        for header in &file.header_lines {
            rows.push(DisplayRow {
                kind: DiffLineKind::Meta,
                old_line: None,
                new_line: None,
                raw: header.clone(),
                code: header.clone(),
            });
        }
        if self.root_document_rendering
            && let Some(mut rendered_rows) = rendered_root_file_rows(file, &self.diff_source)
        {
            rows.append(&mut rendered_rows);
            let highlights = vec![None; rows.len()];
            self.row_cache
                .insert(file_index, CachedFileRows { rows, highlights });
            self.clear_diff_render_cache_for_file(file_index);
            return;
        }
        for hunk in &file.hunks {
            for line in &hunk.lines {
                rows.push(DisplayRow {
                    kind: line.kind.clone(),
                    old_line: line.old_line,
                    new_line: line.new_line,
                    raw: line.raw.clone(),
                    code: line.code.clone(),
                });
            }
        }

        let highlights = vec![None; rows.len()];
        self.row_cache
            .insert(file_index, CachedFileRows { rows, highlights });
        self.clear_diff_render_cache_for_file(file_index);
    }

    pub(crate) fn clear_diff_render_cache(&mut self) {
        self.diff_render_cache.clear();
        self.diff_render_cache_order.clear();
    }

    pub(crate) fn clear_diff_render_cache_for_file(&mut self, file_index: usize) {
        self.diff_render_cache
            .retain(|key, _| key.file_index != file_index);
        self.diff_render_cache_order
            .retain(|key| key.file_index != file_index);
    }

    pub(crate) fn get_diff_render_cache(
        &self,
        key: &DiffRenderCacheKey,
    ) -> Option<&DiffRenderCacheEntry> {
        self.diff_render_cache.get(key)
    }

    pub(crate) fn insert_diff_render_cache(
        &mut self,
        key: DiffRenderCacheKey,
        entry: DiffRenderCacheEntry,
    ) {
        if self.diff_render_cache.contains_key(&key) {
            self.diff_render_cache_order
                .retain(|existing| existing != &key);
        }
        self.diff_render_cache.insert(key.clone(), entry);
        self.diff_render_cache_order.push_back(key);

        while self.diff_render_cache_order.len() > DIFF_RENDER_CACHE_MAX_ENTRIES {
            if let Some(evicted) = self.diff_render_cache_order.pop_front() {
                self.diff_render_cache.remove(&evicted);
            }
        }
    }
}

fn rendered_root_file_rows(file: &DiffFile, diff_source: &DiffSource) -> Option<Vec<DisplayRow>> {
    if !matches!(diff_source, DiffSource::RootDirectory) {
        return None;
    }

    let content = root_file_content(file)?;
    let rendered = if file_has_extension(&file.path, &["json"]) {
        pretty_json_lines(&content)?
    } else if file_has_extension(&file.path, &["md", "markdown", "mdown", "mkd"]) {
        render_markdown_plain_lines(&content)
    } else {
        return None;
    };
    if rendered.is_empty() {
        return None;
    }

    Some(
        rendered
            .into_iter()
            .enumerate()
            .map(|(index, code)| DisplayRow {
                kind: DiffLineKind::Context,
                old_line: None,
                new_line: Some((index + 1) as u32),
                raw: format!(" {code}"),
                code,
            })
            .collect(),
    )
}

fn root_file_content(file: &DiffFile) -> Option<String> {
    let mut lines = Vec::new();
    for hunk in &file.hunks {
        for line in &hunk.lines {
            if !matches!(line.kind, DiffLineKind::Context) {
                return None;
            }
            lines.push(line.code.as_str());
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn file_has_extension(path: &str, extensions: &[&str]) -> bool {
    let Some(extension) = std::path::Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
    else {
        return false;
    };
    extensions
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

fn pretty_json_lines(content: &str) -> Option<Vec<String>> {
    let value = serde_json::from_str::<serde_json::Value>(content).ok()?;
    let pretty = serde_json::to_string_pretty(&value).ok()?;
    Some(pretty.lines().map(ToString::to_string).collect())
}

fn render_markdown_plain_lines(content: &str) -> Vec<String> {
    let mut options = MdOptions::empty();
    options.insert(MdOptions::ENABLE_TABLES);
    options.insert(MdOptions::ENABLE_TASKLISTS);
    options.insert(MdOptions::ENABLE_STRIKETHROUGH);

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut list_stack: Vec<u64> = Vec::new();
    let mut in_code_block = false;

    for event in MdParser::new_ext(content, options) {
        match event {
            MdEvent::Start(tag) => match tag {
                MdTag::Paragraph => {}
                MdTag::Heading { level, .. } => {
                    flush_markdown_plain_line(&mut lines, &mut current);
                    current.push_str(&"#".repeat(heading_level_to_usize(level)));
                    current.push(' ');
                }
                MdTag::List(start) => {
                    list_stack.push(start.unwrap_or(0));
                }
                MdTag::Item => {
                    flush_markdown_plain_line(&mut lines, &mut current);
                    let indent = "  ".repeat(list_stack.len().saturating_sub(1));
                    current.push_str(&indent);
                    if let Some(next) = list_stack.last_mut().filter(|value| **value > 0) {
                        current.push_str(&format!("{next}. "));
                        *next = next.saturating_add(1);
                    } else {
                        current.push_str("- ");
                    }
                }
                MdTag::CodeBlock(_) => {
                    flush_markdown_plain_line(&mut lines, &mut current);
                    in_code_block = true;
                }
                MdTag::BlockQuote(_) => {
                    flush_markdown_plain_line(&mut lines, &mut current);
                    current.push_str("> ");
                }
                MdTag::Table(_) | MdTag::TableHead | MdTag::TableRow => {
                    flush_markdown_plain_line(&mut lines, &mut current);
                }
                MdTag::TableCell => {
                    if !current.is_empty() {
                        current.push_str(" | ");
                    }
                }
                MdTag::Emphasis
                | MdTag::Strong
                | MdTag::Strikethrough
                | MdTag::Link { .. }
                | MdTag::Image { .. }
                | MdTag::HtmlBlock => {}
                _ => {}
            },
            MdEvent::End(tag) => match tag {
                MdTagEnd::Paragraph
                | MdTagEnd::Heading(_)
                | MdTagEnd::Item
                | MdTagEnd::BlockQuote(_)
                | MdTagEnd::TableHead
                | MdTagEnd::TableRow => flush_markdown_plain_line(&mut lines, &mut current),
                MdTagEnd::List(_) => {
                    list_stack.pop();
                    flush_markdown_plain_line(&mut lines, &mut current);
                }
                MdTagEnd::CodeBlock => {
                    in_code_block = false;
                    flush_markdown_plain_line(&mut lines, &mut current);
                }
                MdTagEnd::Table | MdTagEnd::TableCell => {
                    flush_markdown_plain_line(&mut lines, &mut current);
                }
                MdTagEnd::Emphasis
                | MdTagEnd::Strong
                | MdTagEnd::Strikethrough
                | MdTagEnd::Link
                | MdTagEnd::Image
                | MdTagEnd::HtmlBlock
                | MdTagEnd::FootnoteDefinition
                | MdTagEnd::DefinitionList
                | MdTagEnd::DefinitionListTitle
                | MdTagEnd::DefinitionListDefinition
                | MdTagEnd::MetadataBlock(_) => {}
            },
            MdEvent::Text(text)
            | MdEvent::Code(text)
            | MdEvent::InlineMath(text)
            | MdEvent::DisplayMath(text) => {
                if !in_code_block && !current.is_empty() && !current.ends_with([' ', '\n']) {
                    current.push(' ');
                }
                current.push_str(text.trim_matches('\n'));
            }
            MdEvent::SoftBreak => {
                if in_code_block {
                    flush_markdown_plain_line(&mut lines, &mut current);
                } else if !current.ends_with(' ') {
                    current.push(' ');
                }
            }
            MdEvent::HardBreak => {
                flush_markdown_plain_line(&mut lines, &mut current);
            }
            MdEvent::Rule => {
                flush_markdown_plain_line(&mut lines, &mut current);
                lines.push("----".to_string());
            }
            MdEvent::Html(html) | MdEvent::InlineHtml(html) => {
                current.push_str(html.trim());
            }
            MdEvent::TaskListMarker(checked) => {
                current.push_str(if checked { "[x] " } else { "[ ] " });
            }
            MdEvent::FootnoteReference(reference) => {
                current.push_str(&format!("[^{reference}]"));
            }
        }
    }
    flush_markdown_plain_line(&mut lines, &mut current);
    if lines.is_empty() {
        content.lines().map(ToString::to_string).collect()
    } else {
        lines
    }
}

fn flush_markdown_plain_line(lines: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim_end();
    if !trimmed.is_empty() {
        lines.push(trimmed.to_string());
    }
    current.clear();
}

fn heading_level_to_usize(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn inline_comment_editor_reserved_rows(area: Rect) -> usize {
    if area.height < 8 || area.width < 32 {
        return 0;
    }

    let available_width = area.width.saturating_sub(2);
    let available_height = area.height.saturating_sub(1);
    if available_width < 30 || available_height < 6 {
        return 0;
    }

    usize::from(available_height.min(10).saturating_sub(1))
}

#[cfg(test)]
mod root_render_tests {
    use super::*;
    use crate::domain::diff::{DiffHunk, DiffLine};

    fn root_file(path: &str, lines: &[&str]) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            header_lines: Vec::new(),
            hunks: vec![DiffHunk {
                header: "@@ -1,1 +1,1 @@".to_string(),
                old_start: 1,
                old_count: lines.len() as u32,
                new_start: 1,
                new_count: lines.len() as u32,
                lines: lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| DiffLine {
                        kind: DiffLineKind::Context,
                        old_line: Some((index + 1) as u32),
                        new_line: Some((index + 1) as u32),
                        raw: format!(" {line}"),
                        code: (*line).to_string(),
                    })
                    .collect(),
            }],
        }
    }

    #[test]
    fn root_file_rows_are_raw_by_default() {
        let mut app = crate::tui::app::state::tests::make_test_app_with_files_and_comments(
            vec![root_file(
                "config.json",
                &[r#"{"name":"parley","items":[1,2]}"#],
            )],
            vec![],
        )
        .expect("app should build");
        app.diff_source = DiffSource::RootDirectory;

        app.rebuild_row_cache_for_file(0);

        let rows = app
            .row_cache
            .get(&0)
            .expect("rows should be cached")
            .rows
            .iter()
            .map(|row| row.code.as_str())
            .collect::<Vec<_>>();
        assert!(rows.contains(&r#"{"name":"parley","items":[1,2]}"#));
    }

    #[test]
    fn json_root_file_rows_are_pretty_printed_when_rendering_enabled() {
        let file = root_file("config.json", &[r#"{"name":"parley","items":[1,2]}"#]);
        let rows =
            rendered_root_file_rows(&file, &DiffSource::RootDirectory).expect("json should render");
        let rendered = rows
            .iter()
            .map(|row| row.code.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("\"name\": \"parley\""));
        assert!(rendered.contains("\"items\": ["));
    }

    #[test]
    fn markdown_root_file_rows_are_rendered_as_readable_text_when_rendering_enabled() {
        let file = root_file("README.md", &["# Title", "", "- one", "- two"]);
        let rows = rendered_root_file_rows(&file, &DiffSource::RootDirectory)
            .expect("markdown should render");
        let rendered = rows.iter().map(|row| row.code.as_str()).collect::<Vec<_>>();
        assert!(rendered.contains(&"# Title"));
        assert!(rendered.contains(&"- one"));
        assert!(rendered.contains(&"- two"));
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::app::state::tests::{cache_entry, cache_key, make_test_app};
    use anyhow::{Context, Result};

    #[test]
    fn clear_diff_render_cache_for_file_is_scoped() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs"], vec![])?;
        let key_a = cache_key(0);
        let key_b = cache_key(1);
        app.insert_diff_render_cache(key_a.clone(), cache_entry());
        app.insert_diff_render_cache(key_b.clone(), cache_entry());

        app.clear_diff_render_cache_for_file(0);

        assert!(!app.diff_render_cache.contains_key(&key_a));
        assert!(app.diff_render_cache.contains_key(&key_b));
        Ok(())
    }

    #[test]
    fn get_diff_render_cache_returns_cached_entry_by_reference() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"], vec![])?;
        let key = cache_key(0);
        app.insert_diff_render_cache(key.clone(), cache_entry());

        let cached = app
            .get_diff_render_cache(&key)
            .context("cache entry should exist")?;
        let stored = app
            .diff_render_cache
            .get(&key)
            .context("stored entry should exist")?;

        assert!(std::ptr::eq(cached, stored));
        Ok(())
    }
}
