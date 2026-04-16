use std::{io, path::Path, process::Command};

use anyhow::{Context, Result};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};
use time::OffsetDateTime;

use crate::domain::{
    diff::DiffLineKind,
    review::{DiffSide, LineComment},
};

use super::DisplayRow;

pub(super) const MOUSE_WHEEL_SCROLL_LINES: usize = 3;
pub(super) const MOUSE_WHEEL_FILE_SCROLL_FILES: usize = 3;

pub(super) fn comment_matches_display_row(comment: &LineComment, row: &DisplayRow) -> bool {
    if !matches!(
        row.kind,
        DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
    ) {
        return false;
    }

    match comment.side {
        DiffSide::Left => comment.old_line.is_some() && comment.old_line == row.old_line,
        DiffSide::Right => comment.new_line.is_some() && comment.new_line == row.new_line,
    }
}

pub(super) fn format_line_reference(old_line: Option<u32>, new_line: Option<u32>) -> String {
    match (old_line, new_line) {
        (Some(old), Some(new)) => format!("{old}:{new}"),
        (Some(old), None) => format!("{old}:_"),
        (None, Some(new)) => format!("_:{new}"),
        (None, None) => "_:_".to_string(),
    }
}

pub(super) fn format_timestamp_utc(timestamp_ms: u64) -> String {
    let nanos_since_epoch = (timestamp_ms as i128).saturating_mul(1_000_000);
    let dt = OffsetDateTime::from_unix_timestamp_nanos(nanos_since_epoch)
        .expect("timestamp ms should be representable as UTC date-time");
    let month: u8 = dt.month().into();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} UTC",
        dt.year(),
        month,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        dt.millisecond()
    )
}

pub(super) fn slice_chars(input: &str, start: usize, len: usize) -> String {
    if len == 0 {
        return String::new();
    }
    input.chars().skip(start).take(len).collect()
}

pub(super) fn point_in_rect(x: u16, y: u16, rect: Rect) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

pub(super) fn open_log_in_less(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    log_path: &Path,
) -> Result<()> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create log directory {}", parent.display()))?;
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("failed to create/open log file {}", log_path.display()))?;

    disable_raw_mode().context("failed to disable raw mode before launching less")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .context("failed to leave alternate screen before launching less")?;
    terminal.show_cursor().context("failed to show cursor")?;

    let less_result = Command::new("less")
        .arg("+G")
        .arg(log_path)
        .status()
        .with_context(|| format!("failed to launch less for {}", log_path.display()));

    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )
    .context("failed to re-enter alternate screen after less")?;
    enable_raw_mode().context("failed to re-enable raw mode after less")?;
    terminal
        .hide_cursor()
        .context("failed to hide cursor after less")?;
    terminal
        .clear()
        .context("failed to clear terminal after less")?;

    let status = less_result?;
    if !status.success() {
        return Err(anyhow::anyhow!("less exited with status {}", status));
    }
    Ok(())
}

pub(super) fn insert_char_at(text: &mut String, char_index: usize, ch: char) {
    let mut chars: Vec<char> = text.chars().collect();
    let idx = char_index.min(chars.len());
    chars.insert(idx, ch);
    *text = chars.into_iter().collect();
}

pub(super) fn remove_char_at(text: &mut String, char_index: usize) {
    let mut chars: Vec<char> = text.chars().collect();
    if char_index < chars.len() {
        chars.remove(char_index);
        *text = chars.into_iter().collect();
    }
}
