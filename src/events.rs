use crate::message::Message;
use crate::api::Role;
use crate::llm_client::provider::AppProvider;
use crate::message::Content;
use crate::ui::{append_chat_display, append_error, commit_markdown_buffers, reset_textarea};
use crate::{app::App, session::StreamEvent};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};

pub fn handle_user_event<P: AppProvider>(app: &mut App<P>, event: Event) {
    match event {
        Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
            handle_key_event(app, key_event);
        }
        Event::Mouse(mouse_event) => {
            handle_mouse_event(app, mouse_event);
        }
        _ => {}
    }
}
fn handle_mouse_event<P: AppProvider>(app: &mut App<P>, mouse_event: MouseEvent) {
    match mouse_event.kind {
        MouseEventKind::ScrollUp => {
            app.auto_scroll = false;
            app.scroll = app.scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollDown => {
            app.scroll = app.scroll.saturating_add(3);
        }
        _ => {}
    }
}

fn handle_key_event<P: AppProvider>(app: &mut App<P>, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc => app.exit(),
        KeyCode::PageUp => {
            app.auto_scroll = false;
            app.scroll = app.scroll.saturating_sub(5);
        }
        KeyCode::PageDown => {
            app.scroll = app.scroll.saturating_add(5);
        }
        KeyCode::Enter => {
            if app.is_generating {
                return;
            }

            let prompt = app.input_textarea.lines().join("\n");
            if prompt.trim().is_empty() {
                return;
            }

            app.is_generating = true;
            app.was_reasoning = false;
            app.auto_scroll = true;

            reset_textarea(app);

            append_chat_display(app, Role::User, prompt.clone());

            append_chat_display(app, Role::Assistant, String::new());

            let tx_clone = app.network_tx.clone();
            let client_clone = app.llm_client.clone();
            let session_clone = app.session.clone();

            tokio::spawn(async move {
                {
                    let mut session = session_clone.lock().await;
                    session.history.push(Message::User {
                        content: Content::Text(prompt),
                        name: None,
                    });
                }
                let _ = client_clone.run_agent_loop(session_clone, tx_clone).await;
            });
        }
        _ => {
            if !app.is_generating {
                app.input_textarea.input(key_event);
            }
        }
    }
}

pub fn handle_stream_event<P: AppProvider>(app: &mut App<P>, event: StreamEvent) {
    match event {
        StreamEvent::Reasoning(text) => {
            app.active_reasoning_buffer.push_str(&text);
        }
        StreamEvent::Content(text) => {
            app.active_content_buffer.push_str(&text);
        }
        StreamEvent::ToolExecution { name, args } => {
            append_chat_display(
                app,
                Role::Tool,
                format!("\n[System: Executing {} with {}]\n", name, args),
            );
        }
        StreamEvent::PlanUpdated(new_plan) => {
            app.plan = new_plan;
        }
        StreamEvent::Error(err) => {
            append_error(app, &err);
            app.is_generating = false;
            reset_textarea(app);
        }
        StreamEvent::Done => {
            commit_markdown_buffers(app);
            app.is_generating = false;
            reset_textarea(app);
        }
    }
}
