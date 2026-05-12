use super::*;
use crate::domain::diff::DiffFile;
use crate::domain::diff::DiffHunk;
use crate::domain::review::DiffAnchorSnapshot;
use crate::domain::review::SourceAnchorSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedLineAnchor {
    pub(super) side: DiffSide,
    pub(super) old_line: Option<u32>,
    pub(super) new_line: Option<u32>,
    pub(super) line_anchor: LineAnchorSnapshot,
}

impl ResolvedLineAnchor {
    pub(crate) fn from_row(rows: &[DisplayRow], row_index: usize) -> Self {
        let row = &rows[row_index];
        let (side, old_line, new_line) = row_to_comment_anchor(row);
        Self {
            side,
            old_line,
            new_line,
            line_anchor: build_line_anchor_snapshot(rows, row_index),
        }
    }
}

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

pub(crate) fn row_matches_exact_anchor(comment: &LineComment, row: &DisplayRow) -> bool {
    match (comment.old_line, comment.new_line) {
        (Some(old), Some(new)) => row.old_line == Some(old) && row.new_line == Some(new),
        (Some(old), None) => row.old_line == Some(old),
        (None, Some(new)) => row.new_line == Some(new),
        (None, None) => false,
    }
}

pub(crate) fn score_anchor_candidate(
    preferred_side: DiffSide,
    snapshot: &LineAnchorSnapshot,
    rows: &[DisplayRow],
    row_index: usize,
    row: &DisplayRow,
) -> i32 {
    let row_text = normalize_anchor_text(&row.code);
    let target_text = normalize_anchor_text(&snapshot.target_code);
    if row_text.is_empty() || target_text.is_empty() {
        return i32::MIN;
    }

    let mut score = 0;
    if row_text == target_text {
        score += 100;
    } else if normalize_ws(&row_text) == normalize_ws(&target_text) {
        score += 80;
    } else if row_text.contains(&target_text) || target_text.contains(&row_text) {
        score += 40;
    }

    let (before_context, after_context) = row_context_windows(rows, row_index, 2);
    score += score_context_side(&snapshot.before_context, &before_context);
    score += score_context_side(&snapshot.after_context, &after_context);

    if (matches!(preferred_side, DiffSide::Left) && row.old_line.is_some())
        || (matches!(preferred_side, DiffSide::Right) && row.new_line.is_some())
    {
        score += 5;
    }

    score
}

fn score_context_side(expected: &[String], actual: &[String]) -> i32 {
    expected
        .iter()
        .zip(actual.iter())
        .map(|(left, right)| {
            if left == right {
                25
            } else if normalize_ws(left) == normalize_ws(right) {
                10
            } else {
                0
            }
        })
        .sum()
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

fn normalize_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn is_commentable_row(row: &DisplayRow) -> bool {
    matches!(
        row.kind,
        DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
    )
}

pub(crate) fn row_to_comment_anchor(row: &DisplayRow) -> (DiffSide, Option<u32>, Option<u32>) {
    match row.kind {
        DiffLineKind::Added => (DiffSide::Right, None, row.new_line),
        DiffLineKind::Removed => (DiffSide::Left, row.old_line, None),
        DiffLineKind::Context => (DiffSide::Right, row.old_line, row.new_line),
        _ => (DiffSide::Right, None, None),
    }
}

pub(crate) use crate::utils::time::now_ms_utc;
