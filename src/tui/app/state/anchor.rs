use super::*;
use crate::domain::diff::{DiffFile, DiffHunk};
use crate::domain::review::{CommentLineRange, DiffAnchorSnapshot, SourceAnchorSnapshot};
use crate::tui::app::helpers::comment_reference_matches_display_row;

impl TuiApp {
    pub(crate) fn stored_anchor_snapshot_for_row_range(
        &self,
        start_row: usize,
        end_row: usize,
        side: DiffSide,
        old_line: Option<u32>,
        new_line: Option<u32>,
        line_range: Option<CommentLineRange>,
    ) -> Option<StoredAnchorSnapshot> {
        let file = self.current_file()?;
        let rows = self.current_rows();
        let range_start = start_row.min(end_row);
        let range_end = start_row.max(end_row);
        let anchor_row_index = first_commentable_row_index(rows, range_start, range_end)?;
        let anchor_row = rows.get(anchor_row_index)?;
        let selected_text = selected_text_for_rows(rows, range_start, range_end);
        let (before_context, after_context) =
            row_range_context_windows(rows, range_start, range_end, 2);
        let (base_rev, head_rev) = revisions_for_diff_source(&self.diff_source);

        Some(StoredAnchorSnapshot {
            file_path: file.path.clone(),
            side,
            old_line,
            new_line,
            line_range,
            selected_text: selected_text.clone(),
            before_context,
            after_context,
            diff: (!matches!(self.diff_source, DiffSource::RootDirectory))
                .then(|| diff_anchor_snapshot_for_row(file, anchor_row))
                .flatten(),
            source: matches!(self.diff_source, DiffSource::RootDirectory).then(|| {
                SourceAnchorSnapshot {
                    file_content_hash: Some(stable_text_hash(&file_content_text(rows))),
                    selected_text_hash: Some(stable_text_hash(&selected_text)),
                }
            }),
            base_rev,
            head_rev,
        })
    }

    pub(crate) fn refresh_comment_anchor_projections(&mut self) {
        self.comment_anchor_projections.clear();
        let comments = self.review.comments.clone();
        for comment in comments {
            if let Some(projection) = self.exact_anchor_projection_for_comment(&comment) {
                self.comment_anchor_projections
                    .insert(comment.id, projection);
            }
        }
    }

    pub(crate) fn comment_matches_current_projection(
        &self,
        comment: &LineComment,
        row: &DisplayRow,
    ) -> bool {
        if comment.detached {
            return false;
        }
        if let Some(projection) = self.comment_anchor_projection(comment) {
            return projection_matches_row(projection, row);
        }
        comment_reference_matches_display_row(comment, row)
    }

    pub(crate) fn comment_matches_for_navigation(
        &self,
        comment: &LineComment,
        row: &DisplayRow,
    ) -> bool {
        if let Some(projection) = self.comment_anchor_projection(comment) {
            return projection_matches_row(projection, row);
        }
        if comment.detached {
            return comment_reference_matches_display_row(comment, row);
        }
        comment_reference_matches_display_row(comment, row)
    }

    pub(crate) fn comment_line_range_contains_current_projection(
        &self,
        comment: &LineComment,
        row: &DisplayRow,
    ) -> bool {
        let Some(projection) = self.comment_anchor_projection(comment) else {
            return false;
        };
        let Some(range) = projection.line_range.as_ref() else {
            return false;
        };
        line_in_projection_range(row.old_line, range.start_old_line, range.end_old_line)
            || line_in_projection_range(row.new_line, range.start_new_line, range.end_new_line)
    }

    pub(crate) fn projected_comment_reference(&self, comment: &LineComment) -> String {
        self.comment_anchor_projection(comment).map_or_else(
            || format_comment_reference(comment),
            |projection| {
                projection.line_range.as_ref().map_or_else(
                    || format_line_reference(projection.old_line, projection.new_line),
                    format_line_range_reference,
                )
            },
        )
    }

    fn comment_anchor_projection(&self, comment: &LineComment) -> Option<&AnchorProjection> {
        self.comment_anchor_projections.get(&comment.id)
    }

    fn exact_anchor_projection_for_comment(
        &mut self,
        comment: &LineComment,
    ) -> Option<AnchorProjection> {
        let file_path = comment
            .original_anchor
            .as_ref()
            .map_or(comment.file_path.as_str(), |anchor| {
                anchor.file_path.as_str()
            });
        let file_index = self
            .diff
            .files
            .iter()
            .position(|file| file.path == file_path)?;
        self.ensure_row_cache_for_file(file_index);
        let rows = self.row_cache.get(&file_index)?.rows.as_slice();
        let target = projection_target(comment);
        let row_index = if let Some(range) = target.line_range.as_ref() {
            exact_row_range_projection(rows, range, target.selected_text.as_deref())?
        } else {
            exact_single_row_projection(
                rows,
                target.old_line,
                target.new_line,
                target.side,
                target.selected_text.as_deref(),
            )?
        };
        Some(AnchorProjection {
            file_path: file_path.to_string(),
            side: target.side,
            old_line: target.old_line,
            new_line: target.new_line,
            line_range: target.line_range,
            row_index,
        })
    }
}

#[derive(Debug)]
struct ProjectionTarget {
    side: DiffSide,
    old_line: Option<u32>,
    new_line: Option<u32>,
    line_range: Option<CommentLineRange>,
    selected_text: Option<String>,
}

pub(crate) fn build_line_anchor_snapshot(
    rows: &[DisplayRow],
    row_index: usize,
) -> LineAnchorSnapshot {
    let row = &rows[row_index];
    let (before_context, after_context) = row_context_windows(rows, row_index, 2);
    LineAnchorSnapshot {
        target_code: normalize_anchor_text(&row.code),
        before_context,
        after_context,
    }
}

fn row_context_windows(
    rows: &[DisplayRow],
    row_index: usize,
    max_lines: usize,
) -> (Vec<String>, Vec<String>) {
    let mut before = Vec::new();
    let mut cursor = row_index;
    while cursor > 0 && before.len() < max_lines {
        cursor -= 1;
        let row = &rows[cursor];
        if !is_commentable_row(row) {
            continue;
        }
        before.push(normalize_anchor_text(&row.code));
    }

    let mut after = Vec::new();
    let mut cursor = row_index + 1;
    while cursor < rows.len() && after.len() < max_lines {
        let row = &rows[cursor];
        cursor += 1;
        if !is_commentable_row(row) {
            continue;
        }
        after.push(normalize_anchor_text(&row.code));
    }

    (before, after)
}

fn row_range_context_windows(
    rows: &[DisplayRow],
    range_start: usize,
    range_end: usize,
    max_lines: usize,
) -> (Vec<String>, Vec<String>) {
    let mut before = Vec::new();
    let mut cursor = range_start;
    while cursor > 0 && before.len() < max_lines {
        cursor -= 1;
        let row = &rows[cursor];
        if !is_commentable_row(row) {
            continue;
        }
        before.push(normalize_anchor_text(&row.code));
    }

    let mut after = Vec::new();
    let mut cursor = range_end.saturating_add(1);
    while cursor < rows.len() && after.len() < max_lines {
        let row = &rows[cursor];
        cursor += 1;
        if !is_commentable_row(row) {
            continue;
        }
        after.push(normalize_anchor_text(&row.code));
    }

    (before, after)
}

fn projection_target(comment: &LineComment) -> ProjectionTarget {
    if let Some(anchor) = comment.original_anchor.as_ref() {
        return ProjectionTarget {
            side: anchor.side,
            old_line: anchor.old_line,
            new_line: anchor.new_line,
            line_range: anchor.line_range.clone(),
            selected_text: (!anchor.selected_text.is_empty()).then(|| anchor.selected_text.clone()),
        };
    }

    ProjectionTarget {
        side: comment.side,
        old_line: comment.old_line,
        new_line: comment.new_line,
        line_range: comment.line_range.clone(),
        selected_text: None,
    }
}

fn exact_single_row_projection(
    rows: &[DisplayRow],
    old_line: Option<u32>,
    new_line: Option<u32>,
    side: DiffSide,
    selected_text: Option<&str>,
) -> Option<usize> {
    rows.iter()
        .enumerate()
        .find(|(_, row)| {
            row_matches_projection_reference(row, side, old_line, new_line)
                && selected_text_matches_row(row, selected_text)
        })
        .map(|(index, _)| index)
}

fn exact_row_range_projection(
    rows: &[DisplayRow],
    range: &CommentLineRange,
    selected_text: Option<&str>,
) -> Option<usize> {
    let indices = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| row_in_projection_range(row, range).then_some(index))
        .collect::<Vec<_>>();
    let row_index = *indices.last()?;
    let projected_text = indices
        .iter()
        .filter_map(|index| rows.get(*index))
        .map(|row| normalize_anchor_text(&row.code))
        .collect::<Vec<_>>()
        .join("\n");
    if selected_text.is_some_and(|text| normalize_anchor_text(text) != projected_text) {
        return None;
    }
    Some(row_index)
}

fn projection_matches_row(projection: &AnchorProjection, row: &DisplayRow) -> bool {
    if !is_commentable_row(row) {
        return false;
    }
    if let Some(range) = projection.line_range.as_ref() {
        return comment_line_range_end_matches_projection_row(range, row);
    }
    row_matches_projection_reference(
        row,
        projection.side,
        projection.old_line,
        projection.new_line,
    )
}

fn row_matches_projection_reference(
    row: &DisplayRow,
    side: DiffSide,
    old_line: Option<u32>,
    new_line: Option<u32>,
) -> bool {
    if !is_commentable_row(row) {
        return false;
    }
    match (old_line, new_line) {
        (Some(old), Some(new)) => row.old_line == Some(old) && row.new_line == Some(new),
        (Some(old), None) => {
            if matches!(side, DiffSide::Right) {
                false
            } else {
                row.old_line == Some(old)
            }
        }
        (None, Some(new)) => {
            if matches!(side, DiffSide::Left) {
                false
            } else {
                row.new_line == Some(new)
            }
        }
        (None, None) => false,
    }
}

fn selected_text_matches_row(row: &DisplayRow, selected_text: Option<&str>) -> bool {
    selected_text.is_none_or(|text| normalize_anchor_text(text) == normalize_anchor_text(&row.code))
}

fn row_in_projection_range(row: &DisplayRow, range: &CommentLineRange) -> bool {
    line_in_projection_range(row.old_line, range.start_old_line, range.end_old_line)
        || line_in_projection_range(row.new_line, range.start_new_line, range.end_new_line)
}

fn comment_line_range_end_matches_projection_row(
    range: &CommentLineRange,
    row: &DisplayRow,
) -> bool {
    range
        .end_old_line
        .is_some_and(|line| row.old_line == Some(line))
        || range
            .end_new_line
            .is_some_and(|line| row.new_line == Some(line))
}

fn line_in_projection_range(line: Option<u32>, start: Option<u32>, end: Option<u32>) -> bool {
    let Some(line) = line else {
        return false;
    };
    let Some(start) = start else {
        return false;
    };
    let end = end.unwrap_or(start);
    line >= start.min(end) && line <= start.max(end)
}

fn first_commentable_row_index(
    rows: &[DisplayRow],
    range_start: usize,
    range_end: usize,
) -> Option<usize> {
    (range_start..=range_end).find(|row_index| rows.get(*row_index).is_some_and(is_commentable_row))
}

fn selected_text_for_rows(rows: &[DisplayRow], range_start: usize, range_end: usize) -> String {
    rows.iter()
        .enumerate()
        .filter(|(row_index, row)| {
            *row_index >= range_start && *row_index <= range_end && is_commentable_row(row)
        })
        .map(|(_, row)| normalize_anchor_text(&row.code))
        .collect::<Vec<_>>()
        .join("\n")
}

fn file_content_text(rows: &[DisplayRow]) -> String {
    rows.iter()
        .filter(|row| is_commentable_row(row))
        .map(|row| row.code.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn diff_anchor_snapshot_for_row(file: &DiffFile, row: &DisplayRow) -> Option<DiffAnchorSnapshot> {
    let hunk = hunk_for_row(file, row)?;
    Some(DiffAnchorSnapshot {
        hunk_header: hunk.header.clone(),
        hunk_lines: hunk.lines.iter().map(|line| line.raw.clone()).collect(),
    })
}

fn hunk_for_row<'file>(file: &'file DiffFile, row: &DisplayRow) -> Option<&'file DiffHunk> {
    file.hunks.iter().find(|hunk| {
        hunk.lines.iter().any(|line| {
            line.kind == row.kind
                && line.old_line == row.old_line
                && line.new_line == row.new_line
                && line.code == row.code
        })
    })
}

fn revisions_for_diff_source(diff_source: &DiffSource) -> (Option<String>, Option<String>) {
    match diff_source {
        DiffSource::Range { base, head } => (Some(base.clone()), Some(head.clone())),
        DiffSource::Commit { rev } => (None, Some(rev.clone())),
        DiffSource::WorkingTree | DiffSource::RootDirectory => (None, None),
    }
}

fn stable_text_hash(value: &str) -> String {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

fn normalize_anchor_text(value: &str) -> String {
    value.trim().to_string()
}

pub(crate) fn is_commentable_row(row: &DisplayRow) -> bool {
    matches!(
        row.kind,
        DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
    )
}
