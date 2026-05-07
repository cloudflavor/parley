use super::*;

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
