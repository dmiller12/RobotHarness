use crate::api::Role;
use crate::llm_client::provider::AppProvider;
use crate::message::Message;
use crate::message::{Content, ContentBlock, ImageUrlPayload};
use crate::session::TaskStatus;
use crate::ui::{append_chat_display, append_error, commit_markdown_buffers, reset_textarea};
use crate::video::process_and_encode_frame;
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

            let raw_prompt = app.input_textarea.lines().join("\n");
            if raw_prompt.trim().is_empty() {
                return;
            }

            let mut actual_prompt = raw_prompt.trim().to_string();
            let mut is_frame_req = false;
            let mut active_skill = None;

            let evaluator_skill = app.skill_registry.skills.get("evaluator").cloned();

            if actual_prompt.starts_with('/') {
                let parts: Vec<String> = actual_prompt
                    .splitn(2, ' ')
                    .map(|s| s.to_string())
                    .collect();
                let cmd = parts[0].trim_start_matches('/');

                if cmd == "frame" {
                    is_frame_req = true;
                    actual_prompt = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                } else if let Some(skill) = app.skill_registry.skills.get(cmd) {
                    active_skill = Some(skill.clone());
                    actual_prompt = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();

                    // Planners inherently need visual context
                    if cmd == "planner" {
                        is_frame_req = true;
                        // Prepend "Goal: " to match the planner.md prompt constraints
                        actual_prompt = format!("Goal: {}", actual_prompt);
                    }
                }
            }

            app.is_generating = true;
            app.was_reasoning = false;
            app.auto_scroll = true;

            reset_textarea(app);

            append_chat_display(app, Role::User, raw_prompt);

            append_chat_display(app, Role::Assistant, String::new());

            let tx_clone = app.network_tx.clone();
            let client_clone = app.llm_client.clone();
            let session_clone = app.session.clone();
            let frame_rx_clone = app.frame_rx.clone();

            tokio::spawn(async move {
                let is_planner = active_skill
                    .as_ref()
                    .map_or(false, |s| s.metadata.name == "planner");

                let mut message_blocks = vec![ContentBlock::Text {
                    text: actual_prompt,
                }];

                if is_frame_req {
                    let frame_arc = {
                        let rx_lock = frame_rx_clone.borrow();
                        rx_lock.clone()
                    };

                    if let Some(frame) = frame_arc {
                        let base64_image = process_and_encode_frame(frame).await;

                        message_blocks.push(ContentBlock::ImageUrl {
                            image_url: ImageUrlPayload {
                                url: format!("data:image/jpeg;base64,{}", base64_image),
                                detail: None,
                            },
                        });
                    }
                }

                {
                    let mut session = session_clone.lock().await;
                    if let Some(skill) = active_skill {
                        session.history.push(Message::System {
                            content: skill.instructions,
                            name: None,
                        });
                    }
                    session.history.push(Message::User {
                        content: Content::Blocks(message_blocks),
                        name: None,
                    });
                }
                let _ = client_clone
                    .run_agent_loop(session_clone.clone(), tx_clone.clone())
                    .await;
                if is_planner {
                    let eval_session = session_clone.clone();
                    let eval_client = client_clone.clone();
                    let eval_rx = frame_rx_clone.clone();
                    let eval_tx = tx_clone.clone();

                    tokio::spawn(async move {
                        loop {
                            {
                                let session = eval_session.lock().await;
                                if session.plan.is_empty()
                                    || session
                                        .plan
                                        .iter()
                                        .all(|t| t.status == TaskStatus::Completed)
                                {
                                    break;
                                }
                            }

                            let latest_frame_b64 = {
                                let frame_arc = { eval_rx.borrow().clone() };
                                if let Some(frame) = frame_arc {
                                    process_and_encode_frame(frame).await
                                } else {
                                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                    continue;
                                }
                            };

                            {
                                let mut session = eval_session.lock().await;
                                session.prune_intermediate_images();

                                let eval_prompt = evaluator_skill
                                    .as_ref()
                                    .map(|s| s.instructions.clone())
                                    .unwrap_or_else(|| {
                                        "Evaluate the current scene against the plan.".to_string()
                                    });

                                session.history.push(Message::User {
                                    content: Content::Blocks(vec![
                                        ContentBlock::Text { text: eval_prompt },
                                        ContentBlock::ImageUrl {
                                            image_url: ImageUrlPayload {
                                                url: format!(
                                                    "data:image/jpeg;base64,{}",
                                                    latest_frame_b64
                                                ),
                                                detail: None,
                                            },
                                        },
                                    ]),
                                    name: None,
                                });
                            }

                            let _ = eval_client
                                .run_agent_loop(eval_session.clone(), eval_tx.clone())
                                .await;
                        }
                    });
                }
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
        StreamEvent::Usage(usage) => {
            app.last_usage = usage;
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
