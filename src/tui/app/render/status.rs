use std::time::Instant;

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::domain::review::{CommentStatus, ReviewState};
use crate::tui::app::render::helpers::fit_spans_to_width;
use crate::utils::cast::usize_to_u16_saturating;

use super::super::super::theme::ThemeColors;
use super::super::helpers::format_line_reference;
use super::TuiApp;

pub(super) fn compute_status_height(total_height: u16) -> u16 {
    if total_height >= 12 { 4 } else { 3 }
}

pub(super) fn spinner_frame(started_at: Instant) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let idx = ((started_at.elapsed().as_millis() / 100) as usize) % FRAMES.len();
    FRAMES[idx]
}

pub(super) fn draw_status_panel(frame: &mut ratatui::Frame<'_>, app: &TuiApp, area: Rect) {
    let colors = &app.theme().colors;
    let review_state = review_state_label(&app.review.state);
    let file_label = app.current_file().map_or("-", |file| file.path.as_str());
    let file_position = if app.diff.files.is_empty() {
        "0/0".to_string()
    } else {
        format!("{}/{}", app.active_file_index() + 1, app.diff.files.len())
    };

    let selected_thread = app.selected_comment_details().map_or_else(
        || ("none".to_string(), Style::default().fg(colors.text_muted)),
        |comment| {
            (
                format!(
                    "#{} {} {}",
                    comment.id,
                    format_line_reference(comment.old_line, comment.new_line),
                    comment_status_label(&comment.status)
                ),
                comment_status_style(&comment.status, colors),
            )
        },
    );
    let open_threads = app
        .review
        .comments
        .iter()
        .filter(|comment| matches!(comment.status, CommentStatus::Open))
        .count();
    let pending_human_count = app
        .review
        .comments
        .iter()
        .filter(|comment| matches!(comment.status, CommentStatus::Pending))
        .count();

    let inner_width = usize::from(area.width.saturating_sub(2)).max(1);
    let line_1 = build_status_field_line(
        &[
            (
                "review",
                format!("{}:{review_state}", app.review.name),
                review_state_style(&app.review.state, colors),
            ),
            (
                "file",
                format!("{file_position} {file_label}"),
                Style::default().fg(colors.text_primary),
            ),
            ("thread", selected_thread.0, selected_thread.1),
            (
                "counts",
                format!("open {open_threads} pending {pending_human_count}"),
                Style::default().fg(colors.text_primary),
            ),
        ],
        inner_width,
        colors,
    );
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let secondary_left = status_footer_context(app);
    let line_2_right = format!(
        "user {} · ai {} · ? help · {version}",
        app.config.user_name,
        app.ai_provider.as_str()
    );
    let line_2 = build_right_tag_line(
        &secondary_left,
        &line_2_right,
        inner_width,
        Style::default().fg(colors.status_help),
    );

    let inner_height = usize::from(area.height.saturating_sub(2));
    let panel_lines = if inner_height >= 2 {
        vec![line_1, line_2]
    } else {
        vec![line_1]
    };

    let panel = Paragraph::new(panel_lines).block(
        Block::default()
            .title("Review")
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
            .border_style(Style::default().fg(colors.thread_border))
            .title_style(
                Style::default()
                    .fg(colors.accent)
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(panel, area);
}

pub(super) fn draw_status_toast(frame: &mut ratatui::Frame<'_>, app: &TuiApp, status_area: Rect) {
    let Some(message) = app.status_toast_message.as_ref() else {
        return;
    };
    if app
        .status_toast_until
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        return;
    }

    let root = frame.area();
    let colors = app.theme().colors.clone();
    let max_text_width = usize::from(root.width.saturating_sub(10)).clamp(12, 46);
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return;
    }
    let text = truncate_with_ellipsis(trimmed, max_text_width);
    let popup_width = usize_to_u16_saturating(text.chars().count())
        .saturating_add(2)
        .min(root.width.saturating_sub(4));
    let x = root
        .x
        .saturating_add(root.width.saturating_sub(popup_width).saturating_sub(2));
    let y = status_area.y.saturating_sub(1).max(root.y);
    let area = Rect {
        x,
        y,
        width: popup_width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            format!(" {text} "),
            Style::default()
                .bg(colors.selected_line_bg)
                .fg(colors.status_help),
        )])),
        area,
    );
}

pub(super) fn build_status_field_line(
    fields: &[(&str, String, Style)],
    width: usize,
    colors: &ThemeColors,
) -> Line<'static> {
    let mut spans = Vec::new();
    let label_style = Style::default()
        .fg(colors.status_help)
        .add_modifier(Modifier::BOLD);
    let separator_style = Style::default().fg(colors.thread_border);
    for (index, (label, value, value_style)) in fields.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  |  ", separator_style));
        }
        spans.push(Span::styled(format!("{label} "), label_style));
        spans.push(Span::styled(value.clone(), *value_style));
    }

    Line::from(fit_spans_to_width(
        spans,
        width,
        Style::default().fg(colors.status_help),
    ))
}

pub(super) fn truncate_with_ellipsis(input: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let input_len = input.chars().count();
    if input_len <= max_len {
        return input.to_string();
    }
    if max_len == 1 {
        return "…".to_string();
    }
    let mut out: String = input.chars().take(max_len - 1).collect();
    out.push('…');
    out
}

fn status_footer_context(app: &TuiApp) -> String {
    if app.shortcuts_modal_visible {
        "Help open · tab switch docs · esc close".to_string()
    } else if app.command_palette.is_some() {
        "Command palette · enter run · esc close".to_string()
    } else if app.theme_picker.is_some() {
        "Theme picker · enter apply · esc close".to_string()
    } else if app.commit_picker.is_some() {
        "Commit picker · type sha/message · enter apply".to_string()
    } else if app.review_picker.is_some() {
        "Review picker · type name/state · enter apply".to_string()
    } else if app.settings_editor.is_some() {
        "Settings · enter save · esc cancel".to_string()
    } else if app.command_prompt.is_some() {
        "Command prompt · enter run · esc cancel".to_string()
    } else if app.file_search.focused {
        "File filter · type to narrow · esc close".to_string()
    } else if let Some(inline) = app.inline_comment.as_ref() {
        if inline.preview_mode {
            "Comment preview · ctrl+p edit · ctrl+s save".to_string()
        } else {
            "Comment draft · ctrl+s save · ctrl+p preview".to_string()
        }
    } else if app.ai_task.is_some() {
        "AI running · k cancel · h stream · l logs".to_string()
    } else {
        String::new()
    }
}

pub(super) fn theme_variant_label(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.contains("light") {
        "light"
    } else if lower.contains("dark") {
        "dark"
    } else {
        "mixed"
    }
}

pub(super) fn theme_family_label(name: &str) -> &str {
    name.split(['_', '-'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(name)
}

pub(super) fn comment_status_label(status: &CommentStatus) -> &'static str {
    match status {
        CommentStatus::Open => "open",
        CommentStatus::Pending => "pending human",
        CommentStatus::Addressed => "addressed",
    }
}

pub(super) fn comment_status_style(status: &CommentStatus, colors: &ThemeColors) -> Style {
    let color = match status {
        CommentStatus::Open => colors.removed_sign,
        CommentStatus::Pending => colors.accent,
        CommentStatus::Addressed => colors.added_sign,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub(super) fn review_state_label(state: &ReviewState) -> &'static str {
    match state {
        ReviewState::Open => "open",
        ReviewState::UnderReview => "under_review",
        ReviewState::Done => "done",
    }
}

fn review_state_style(state: &ReviewState, colors: &ThemeColors) -> Style {
    match state {
        ReviewState::Open => Style::default()
            .fg(colors.accent)
            .add_modifier(Modifier::BOLD),
        ReviewState::UnderReview => Style::default()
            .fg(colors.hunk_header)
            .add_modifier(Modifier::BOLD),
        ReviewState::Done => Style::default()
            .fg(colors.added_sign)
            .add_modifier(Modifier::BOLD),
    }
}

fn build_right_tag_line(
    left: &str,
    right: &str,
    width: usize,
    right_style: Style,
) -> Line<'static> {
    if width == 0 {
        return Line::from("");
    }

    let right_len = right.chars().count();
    if right_len >= width {
        return Line::from(Span::styled(
            right.chars().take(width).collect::<String>(),
            right_style,
        ));
    }

    let max_left_len = width.saturating_sub(right_len + 1);
    let clipped_left: String = left.chars().take(max_left_len).collect();
    let gap_len = width
        .saturating_sub(clipped_left.chars().count())
        .saturating_sub(right_len);

    Line::from(vec![
        Span::raw(clipped_left),
        Span::raw(" ".repeat(gap_len)),
        Span::styled(right.to_string(), right_style),
    ])
}
