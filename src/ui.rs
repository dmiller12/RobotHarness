use ratatui::widgets::Wrap;
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Style, Modifier},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use ratatui_textarea::TextArea;
use tui_markdown::from_str;

use crate::api::Role;
use crate::app::App;
use crate::llm_client::provider::AppProvider;
use crate::session::TaskStatus;

pub fn draw<P: AppProvider>(app: &mut App<P>, frame: &mut Frame) {
    let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).split(frame.area());

    let main_chunks = Layout::horizontal([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(chunks[0]); // inside your existing vertical layout

    let history_area = main_chunks[0];

    // Combine static history with live unformatted buffers
    let mut display_lines = app.chat_display.clone();

    if !app.active_reasoning_buffer.is_empty() {
        display_lines.push(Line::styled(
            format!("Thinking: {}", app.active_reasoning_buffer),
            Style::default().fg(Color::DarkGray),
        ));
    }

    if !app.active_content_buffer.is_empty() {
        // Split raw lines so explicit newlines work during streaming
        for raw_line in app.active_content_buffer.lines() {
            display_lines.push(Line::raw(raw_line.to_string()));
        }
    }

    // Calculate total lines for scrolling
    let inner_width = history_area.width.saturating_sub(2).max(1) as usize;
    let total_lines: u16 = display_lines
        .iter()
        .map(|line| {
            let len = line.width();
            (len.saturating_sub(1) / inner_width) as u16 + 1
        })
        .sum();

    let history_height = history_area.height.saturating_sub(2);
    let max_scroll = total_lines.saturating_sub(history_height);

    if app.auto_scroll {
        app.scroll = max_scroll;
    } else {
        app.scroll = app.scroll.min(max_scroll);
        if app.scroll == max_scroll {
            app.auto_scroll = true;
        }
    }

    let latency_part = app.last_latency.as_ref().map_or(String::new(), |m| {
        format!(" | End-to-end: {}ms", m.as_millis())
    });

    let metrics_part = app.last_metrics.as_ref().map_or(String::new(), |m| {
        format!(" | TTFT: {}ms | {:.1} TPS", m.ttft_ms, m.tps)
    });
    let footer_text = format!(
        " In: {} | Out: {} | Total: {}{}{} ",
        app.last_usage.prompt_tokens,
        app.last_usage.completion_tokens,
        app.last_usage.total_tokens,
        latency_part,
        metrics_part,
    );

    let history_block = Block::bordered()
        .title(Line::from(" Robot Harness ").centered())
        .title_bottom(
            Line::from(Span::styled(
                footer_text,
                Style::default().fg(Color::DarkGray),
            ))
            .right_aligned(),
        )
        .border_set(border::THICK);

    let mut plan_items: Vec<Line> = Vec::new();
    if let Some(goal) = &app.goal {
        plan_items.push(Line::from(vec![
            Span::styled(goal, Style::default().fg(Color::Cyan)),
        ]));
        // Add a spacer line between the goal and the task list
        plan_items.push(Line::raw(""));
    }
    plan_items.extend(app
        .plan
        .iter()
        .map(|task| {
            let prefix = match task.status {
                TaskStatus::Completed => "[x] ",
                TaskStatus::InProgress => "[>] ",
                TaskStatus::Pending => "[ ] ",
            };
            Line::raw(format!("{}{}", prefix, task.description))
        })
    );

    let plan_block = Block::bordered().title(" Plan ").border_set(border::THICK);
    frame.render_widget(Paragraph::new(plan_items).block(plan_block), main_chunks[1]);

    frame.render_widget(
        Paragraph::new(display_lines)
            .block(history_block)
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0)),
        history_area,
    );

    frame.render_widget(&app.input_textarea, chunks[1]);
}

pub fn reset_textarea<P: AppProvider>(app: &mut App<P>) {
    let mut textarea = TextArea::default();
    textarea.set_block(
        Block::bordered()
            .title(" Prompt ")
            .border_set(border::THICK),
    );
    textarea.set_cursor_line_style(Style::default());
    app.input_textarea = textarea;
}

pub fn append_chat_display<P: AppProvider>(app: &mut App<P>, role: Role, message: String) {
    let (prefix, color) = match role {
        Role::User => ("User: ", Color::Blue),
        Role::Assistant => ("Assistant: ", Color::Green),
        Role::System => ("System: ", Color::Yellow),
        Role::Tool => ("Tool: ", Color::Cyan),
        Role::Info => ("INFO: ", Color::Gray),
    };

    let mut spans = vec![Span::styled(prefix, Style::default().fg(color).bold())];

    if !message.is_empty() {
        spans.push(Span::raw(message));
    }

    app.chat_display.push(Line::from(spans));
}

pub fn append_error<P: AppProvider>(app: &mut App<P>, err: &str) {
    app.chat_display.push(Line::from(vec![
        Span::styled("Error: ", Style::default().fg(Color::Red).bold()),
        Span::raw(err.to_string()),
    ]));
}

fn append_parsed_markdown(
    display: &mut Vec<Line<'static>>,
    buffer: &mut String,
    override_color: Option<Color>,
) {
    if buffer.is_empty() {
        return;
    }

    let parsed_text = from_str(buffer);
    let lines: Vec<Line<'static>> = parsed_text
        .lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| {
                    let style = match override_color {
                        Some(c) => span.style.fg(c),
                        None => span.style,
                    };
                    Span::styled(span.content.into_owned(), style)
                })
                .collect();
            Line::from(owned_spans)
        })
        .collect();

    display.extend(lines);
    buffer.clear();
}

pub fn commit_markdown_buffers<P: AppProvider>(app: &mut App<P>) {
    append_parsed_markdown(
        &mut app.chat_display,
        &mut app.active_reasoning_buffer,
        Some(Color::DarkGray),
    );
    append_parsed_markdown(&mut app.chat_display, &mut app.active_content_buffer, None);
    app.chat_display.push(Line::default());
}
