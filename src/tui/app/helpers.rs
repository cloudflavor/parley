use super::DisplayRow;
use crate::domain::diff::DiffLineKind;
use crate::domain::review::{CommentLineRange, DiffSide, LineComment};
use anyhow::{Context, Result};
use crossterm::event::DisableMouseCapture;
use crossterm::event::EnableMouseCapture;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use time::{OffsetDateTime, UtcOffset};

pub(super) const MOUSE_WHEEL_SCROLL_LINES: usize = 3;
pub(super) const MOUSE_WHEEL_FILE_SCROLL_FILES: usize = 3;

pub(super) fn comment_reference_matches_display_row(
    comment: &LineComment,
    row: &DisplayRow,
) -> bool {
    if !matches!(
        row.kind,
        DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
    ) {
        return false;
    }

    if let Some(range) = comment.line_range.as_ref() {
        return comment_line_range_end_matches_display_row(range, row);
    }

    match (comment.old_line, comment.new_line) {
        (Some(old), Some(new)) => row.old_line == Some(old) && row.new_line == Some(new),
        (Some(old), None) => {
            if matches!(comment.side, DiffSide::Right) {
                false
            } else {
                row.old_line == Some(old)
            }
        }
        (None, Some(new)) => {
            if matches!(comment.side, DiffSide::Left) {
                false
            } else {
                row.new_line == Some(new)
            }
        }
        (None, None) => false,
    }
}

fn comment_line_range_end_matches_display_row(range: &CommentLineRange, row: &DisplayRow) -> bool {
    range
        .end_old_line
        .is_some_and(|line| row.old_line == Some(line))
        || range
            .end_new_line
            .is_some_and(|line| row.new_line == Some(line))
}

pub(super) fn format_line_reference(old_line: Option<u32>, new_line: Option<u32>) -> String {
    match (old_line, new_line) {
        (Some(old), Some(new)) => format!("{old}:{new}"),
        (Some(old), None) => format!("{old}:_"),
        (None, Some(new)) => format!("_:{new}"),
        (None, None) => "_:_".to_string(),
    }
}

pub(super) fn format_line_range_reference(range: &CommentLineRange) -> String {
    format!(
        "{}:{}",
        format_optional_line_range(range.start_old_line, range.end_old_line),
        format_optional_line_range(range.start_new_line, range.end_new_line)
    )
}

pub(super) fn format_comment_reference(comment: &LineComment) -> String {
    comment.line_range.as_ref().map_or_else(
        || format_line_reference(comment.old_line, comment.new_line),
        format_line_range_reference,
    )
}

fn format_optional_line_range(start: Option<u32>, end: Option<u32>) -> String {
    match (start, end) {
        (Some(start), Some(end)) if start != end => format!("{start}-{end}"),
        (Some(start), _) => start.to_string(),
        (None, Some(end)) => end.to_string(),
        (None, None) => "_".to_string(),
    }
}

pub(super) fn format_timestamp_utc(timestamp_ms: u64) -> String {
    let nanos_since_epoch = (timestamp_ms as i128).saturating_mul(1_000_000);
    let utc_dt = match OffsetDateTime::from_unix_timestamp_nanos(nanos_since_epoch) {
        Ok(dt) => dt,
        Err(_) => return "invalid timestamp".to_string(),
    };
    let local_dt = local_utc_offset().map_or(utc_dt, |offset| utc_dt.to_offset(offset));
    let month: u8 = local_dt.month().into();
    let offset_seconds = local_dt.offset().whole_seconds();
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let abs_offset_seconds = offset_seconds.abs();
    let offset_hours = abs_offset_seconds / 3600;
    let offset_minutes = (abs_offset_seconds % 3600) / 60;
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} UTC{}{offset_hours:02}:{offset_minutes:02}",
        local_dt.year(),
        month,
        local_dt.day(),
        local_dt.hour(),
        local_dt.minute(),
        local_dt.second(),
        local_dt.millisecond(),
        sign
    )
}

fn local_utc_offset() -> Option<UtcOffset> {
    static OFFSET_SECONDS: OnceLock<Option<i32>> = OnceLock::new();
    let seconds = OFFSET_SECONDS
        .get_or_init(|| {
            let output = Command::new("date").arg("+%z").output().ok()?;
            if !output.status.success() {
                return None;
            }
            let raw = String::from_utf8(output.stdout).ok()?;
            parse_utc_offset_seconds(raw.trim())
        })
        .to_owned()?;
    UtcOffset::from_whole_seconds(seconds).ok()
}

fn parse_utc_offset_seconds(raw: &str) -> Option<i32> {
    if raw.len() != 5 {
        return None;
    }
    let sign = match raw.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hours: i32 = raw[1..3].parse().ok()?;
    let minutes: i32 = raw[3..5].parse().ok()?;
    Some(sign * (hours * 3600 + minutes * 60))
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

pub(super) fn suspend_tui_process(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mouse_capture_enabled: bool,
) -> Result<()> {
    run_with_terminal_released(
        terminal,
        mouse_capture_enabled,
        "suspend",
        "suspend",
        suspend_current_process,
    )
}

pub(super) fn open_file_in_pager(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mouse_capture_enabled: bool,
    path: &Path,
) -> Result<()> {
    run_with_terminal_released(
        terminal,
        mouse_capture_enabled,
        "opening pager",
        "pager",
        || run_pager(path),
    )
}

fn run_with_terminal_released(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mouse_capture_enabled: bool,
    before_label: &str,
    after_label: &str,
    action: impl FnOnce() -> Result<()>,
) -> Result<()> {
    disable_raw_mode()
        .with_context(|| format!("failed to disable raw mode before {before_label}"))?;
    leave_terminal_screen(terminal, mouse_capture_enabled)
        .with_context(|| format!("failed to leave alternate screen before {before_label}"))?;
    terminal
        .show_cursor()
        .with_context(|| format!("failed to show cursor before {before_label}"))?;

    let action_result = action();

    enter_terminal_screen(terminal, mouse_capture_enabled)
        .with_context(|| format!("failed to re-enter alternate screen after {after_label}"))?;
    enable_raw_mode()
        .with_context(|| format!("failed to re-enable raw mode after {after_label}"))?;
    terminal
        .hide_cursor()
        .with_context(|| format!("failed to hide cursor after {after_label}"))?;
    terminal
        .clear()
        .with_context(|| format!("failed to clear terminal after {after_label}"))?;

    action_result
}

fn leave_terminal_screen(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mouse_capture_enabled: bool,
) -> Result<()> {
    if mouse_capture_enabled {
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
    } else {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }
    Ok(())
}

fn enter_terminal_screen(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mouse_capture_enabled: bool,
) -> Result<()> {
    if mouse_capture_enabled {
        execute!(
            terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture
        )?;
    } else {
        execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    }
    Ok(())
}

fn run_pager(path: &Path) -> Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg("exec ${PAGER:-less -N -R -S} \"$1\"")
        .arg("parley-pager")
        .arg(path)
        .status()
        .with_context(|| format!("failed to launch pager for {}", path.display()))?;
    if !status.success() {
        return Err(anyhow::anyhow!("pager exited with status {status}"));
    }
    Ok(())
}

#[cfg(unix)]
fn suspend_current_process() -> Result<()> {
    let pid = std::process::id().to_string();
    let status = Command::new("kill")
        .arg("-TSTP")
        .arg(pid)
        .status()
        .context("failed to invoke kill -TSTP")?;
    if !status.success() {
        return Err(anyhow::anyhow!("kill -TSTP exited with status {status}"));
    }
    Ok(())
}

#[cfg(not(unix))]
fn suspend_current_process() -> Result<()> {
    Err(anyhow::anyhow!(
        "suspend is unsupported on this platform; use SIGTSTP on Unix"
    ))
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

pub(super) fn apply_single_line_edit_key(
    text: &mut String,
    cursor_col: &mut usize,
    key: KeyEvent,
) -> bool {
    match key.code {
        KeyCode::Left => {
            *cursor_col = cursor_col.saturating_sub(1);
            true
        }
        KeyCode::Right => {
            *cursor_col = (*cursor_col + 1).min(text.chars().count());
            true
        }
        KeyCode::Home => {
            *cursor_col = 0;
            true
        }
        KeyCode::End => {
            *cursor_col = text.chars().count();
            true
        }
        KeyCode::Backspace if *cursor_col > 0 => {
            remove_char_at(text, *cursor_col - 1);
            *cursor_col -= 1;
            true
        }
        KeyCode::Delete if *cursor_col < text.chars().count() => {
            remove_char_at(text, *cursor_col);
            true
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            insert_char_at(text, *cursor_col, ch);
            *cursor_col += 1;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_utc_offset_seconds;

    #[test]
    fn parses_positive_utc_offset() {
        assert_eq!(parse_utc_offset_seconds("+0200"), Some(2 * 3600));
        assert_eq!(parse_utc_offset_seconds("+0530"), Some(5 * 3600 + 30 * 60));
    }

    #[test]
    fn parses_negative_utc_offset() {
        assert_eq!(parse_utc_offset_seconds("-0700"), Some(-7 * 3600));
        assert_eq!(
            parse_utc_offset_seconds("-0330"),
            Some(-(3 * 3600 + 30 * 60))
        );
    }

    #[test]
    fn rejects_invalid_utc_offset() {
        assert_eq!(parse_utc_offset_seconds(""), None);
        assert_eq!(parse_utc_offset_seconds("0200"), None);
        assert_eq!(parse_utc_offset_seconds("+2"), None);
        assert_eq!(parse_utc_offset_seconds("+25AA"), None);
    }
}
