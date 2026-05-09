use std::{io, path::Path, process::Command, sync::OnceLock};

use anyhow::{Context, Result};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};
use time::{OffsetDateTime, UtcOffset};

use crate::domain::{
    diff::DiffLineKind,
    review::{DiffSide, LineComment},
};

use super::DisplayRow;

pub(super) const MOUSE_WHEEL_SCROLL_LINES: usize = 3;
pub(super) const MOUSE_WHEEL_FILE_SCROLL_FILES: usize = 3;

pub(super) fn comment_matches_display_row(comment: &LineComment, row: &DisplayRow) -> bool {
    if comment.detached {
        return false;
    }

    if !matches!(
        row.kind,
        DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
    ) {
        return false;
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

pub(super) fn open_log_in_less(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    log_path: &Path,
    mouse_capture_enabled: bool,
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
    if mouse_capture_enabled {
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )
        .context("failed to leave alternate screen before launching less")?;
    } else {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)
            .context("failed to leave alternate screen before launching less")?;
    }
    terminal.show_cursor().context("failed to show cursor")?;

    let less_result = Command::new("less")
        .arg("+G")
        .arg(log_path)
        .status()
        .with_context(|| format!("failed to launch less for {}", log_path.display()));

    if mouse_capture_enabled {
        execute!(
            terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture
        )
        .context("failed to re-enter alternate screen after less")?;
    } else {
        execute!(terminal.backend_mut(), EnterAlternateScreen)
            .context("failed to re-enter alternate screen after less")?;
    }
    enable_raw_mode().context("failed to re-enable raw mode after less")?;
    terminal
        .hide_cursor()
        .context("failed to hide cursor after less")?;
    terminal
        .clear()
        .context("failed to clear terminal after less")?;

    let status = less_result?;
    if !status.success() {
        return Err(anyhow::anyhow!("less exited with status {status}"));
    }
    Ok(())
}

pub(super) fn suspend_tui_process(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mouse_capture_enabled: bool,
) -> Result<()> {
    disable_raw_mode().context("failed to disable raw mode before suspend")?;
    if mouse_capture_enabled {
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )
        .context("failed to leave alternate screen before suspend")?;
    } else {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)
            .context("failed to leave alternate screen before suspend")?;
    }
    terminal.show_cursor().context("failed to show cursor")?;

    let suspend_result = suspend_current_process();

    if mouse_capture_enabled {
        execute!(
            terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture
        )
        .context("failed to re-enter alternate screen after suspend")?;
    } else {
        execute!(terminal.backend_mut(), EnterAlternateScreen)
            .context("failed to re-enter alternate screen after suspend")?;
    }
    enable_raw_mode().context("failed to re-enable raw mode after suspend")?;
    terminal
        .hide_cursor()
        .context("failed to hide cursor after suspend")?;
    terminal
        .clear()
        .context("failed to clear terminal after suspend")?;

    suspend_result
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

#[cfg(test)]
mod tests {
    use super::{DisplayRow, comment_matches_display_row, parse_utc_offset_seconds};
    use crate::domain::{
        diff::DiffLineKind,
        review::{Author, CommentStatus, DiffSide, LineComment},
    };

    fn make_row(kind: DiffLineKind, old_line: Option<u32>, new_line: Option<u32>) -> DisplayRow {
        DisplayRow {
            kind,
            old_line,
            new_line,
            raw: String::new(),
            code: String::new(),
        }
    }

    fn make_comment(side: DiffSide, old_line: Option<u32>, new_line: Option<u32>) -> LineComment {
        LineComment {
            id: 1,
            file_path: "src/lib.rs".to_string(),
            old_line,
            new_line,
            side,
            line_anchor: None,
            detached: false,
            body: "x".to_string(),
            author: Author::User,
            status: CommentStatus::Open,
            replies: Vec::new(),
            created_at_ms: 0,
            updated_at_ms: 0,
            addressed_at_ms: None,
        }
    }

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

    #[test]
    fn anchor_with_both_lines_prefers_exact_pair() {
        let comment = make_comment(DiffSide::Right, Some(8), Some(7));
        let exact = make_row(DiffLineKind::Context, Some(8), Some(7));
        assert!(comment_matches_display_row(&comment, &exact));
    }

    #[test]
    fn anchor_with_both_lines_requires_exact_pair_after_shift() {
        let comment = make_comment(DiffSide::Right, Some(8), Some(7));
        let shifted = make_row(DiffLineKind::Context, Some(8), Some(10));
        assert!(!comment_matches_display_row(&comment, &shifted));
    }

    #[test]
    fn anchor_with_both_lines_does_not_match_new_line_only() {
        let comment = make_comment(DiffSide::Right, Some(8), Some(7));
        let wrong_context = make_row(DiffLineKind::Context, Some(5), Some(7));
        assert!(!comment_matches_display_row(&comment, &wrong_context));
    }
}
