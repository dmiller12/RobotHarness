use crate::api::Role;
use crate::llm_client::openai::ReasoningEffort;
use crate::llm_client::provider::AppProvider;
use crate::message::Message;
use crate::message::{Content, ContentBlock, ImageUrlPayload};
use crate::session::{SessionData, TaskStatus};
use crate::skill::{Skill, SkillRegistry};
use crate::ui::{append_chat_display, append_error, commit_markdown_buffers, reset_textarea};
use crate::video::process_and_encode_frame;
use crate::{app::App, session::StreamEvent};

use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    None,
    Reasoning,
    Content,
    Tool,
    Other,
}
pub enum UserAction {
    Standard {
        prompt: String,
        skill: Option<Skill>,
    },
    Planner {
        goal: String,
        skill: Skill,
    },
    Eval,
    Frame {
        prompt: String,
    },
}

impl UserAction {
    pub fn parse(raw_prompt: &str, registry: &SkillRegistry) -> Self {
        let prompt = raw_prompt.trim().to_string();

        if !prompt.starts_with('/') {
            return UserAction::Standard {
                prompt,
                skill: None,
            };
        }

        let parts: Vec<&str> = prompt.splitn(2, ' ').collect();
        let cmd = parts[0].trim_start_matches('/');
        let args = parts.get(1).unwrap_or(&"").trim().to_string();

        match cmd {
            "eval" => UserAction::Eval,
            "frame" => UserAction::Frame { prompt: args },
            "planner" => {
                if let Some(skill) = registry.skills.get(cmd).cloned() {
                    UserAction::Planner {
                        goal: format!("Goal: {}", args),
                        skill,
                    }
                } else {
                    UserAction::Standard {
                        prompt: args,
                        skill: None,
                    }
                }
            }
            _ => {
                if let Some(skill) = registry.skills.get(cmd).cloned() {
                    UserAction::Standard {
                        prompt: args,
                        skill: Some(skill),
                    }
                } else {
                    // Fallback if the command is unrecognized
                    UserAction::Standard {
                        prompt,
                        skill: None,
                    }
                }
            }
        }
    }
}

fn handle_submission<P: AppProvider>(app: &mut App<P>, action: UserAction) {
    app.is_generating = true;
    app.was_reasoning = false;
    app.auto_scroll = true;
    reset_textarea(app);

    match action {
        UserAction::Eval => spawn_eval_loop(app),
        UserAction::Standard { prompt, skill } => spawn_agent_task(app, prompt, skill, false),
        UserAction::Planner { goal, skill } => spawn_agent_task(app, goal, Some(skill), true),
        UserAction::Frame { prompt } => spawn_agent_task(app, prompt, None, true),
    }
}

fn spawn_agent_task<P: AppProvider>(
    app: &mut App<P>,
    prompt: String,
    skill: Option<Skill>,
    requires_frame: bool,
) {
    let tx_clone = app.network_tx.clone();
    let client_clone = app.llm_client.clone();
    let session_clone = app.session.clone();
    let frame_rx_clone = app.frame_rx.clone();

    tokio::spawn(async move {
        let mut initial_frame_b64 = String::new();

        if requires_frame {
            let frame_arc = {
                let rx_lock = frame_rx_clone.borrow();
                rx_lock.clone()
            };

            if let Some(timestamped_frame) = frame_arc {
                initial_frame_b64 = process_and_encode_frame(timestamped_frame.buffer).await;
            }
        }

        let mut message_blocks = vec![ContentBlock::Text { text: prompt }];

        if !initial_frame_b64.is_empty() {
            message_blocks.push(ContentBlock::ImageUrl {
                image_url: ImageUrlPayload {
                    url: format!("data:image/jpeg;base64,{}", initial_frame_b64),
                    detail: None,
                },
            });
        }

        {
            let mut session = session_clone.lock().await;
            if let Some(skill) = skill {
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
            .run_agent_loop(
                session_clone.clone(),
                tx_clone.clone(),
                Some(ReasoningEffort::High),
            )
            .await;
    });
}

fn spawn_eval_loop<P: AppProvider>(app: &mut App<P>) {
    let tx_clone = app.network_tx.clone();
    let client_clone = app.llm_client.clone();
    let session_clone = app.session.clone();
    let frame_rx_clone = app.frame_rx.clone();
    let evaluator_skill = app.skill_registry.skills.get("evaluator").cloned();

    let handle = tokio::spawn(async move {
        let mut initial_frame_b64 = String::new();
        let frame_arc = {
            let rx_lock = frame_rx_clone.borrow();
            rx_lock.clone()
        };

        if let Some(timestamped_frame) = frame_arc {
            initial_frame_b64 = process_and_encode_frame(timestamped_frame.buffer).await;
        }

        let initial_plan = { session_clone.lock().await.plan.clone() };
        let eval_prompt = evaluator_skill
            .as_ref()
            .map(|s| s.instructions.clone())
            .unwrap_or_else(|| "Evaluate the current scene against the plan.".to_string());

        let mut isolated_eval_session = SessionData::new(&eval_prompt);
        isolated_eval_session.plan = initial_plan;

        if !initial_frame_b64.is_empty() {
            isolated_eval_session.history.push(Message::User {
                content: Content::Blocks(vec![
                    ContentBlock::Text {
                        text: "Initial Image".to_string(),
                    },
                    ContentBlock::ImageUrl {
                        image_url: ImageUrlPayload {
                            url: format!("data:image/jpeg;base64,{}", initial_frame_b64),
                            detail: None,
                        },
                    },
                ]),
                name: None,
            });
        }

        let eval_session_arc = Arc::new(tokio::sync::Mutex::new(isolated_eval_session));

        loop {
            let plan = { session_clone.lock().await.plan.clone() };
            if plan.is_empty() || plan.iter().all(|t| t.status == TaskStatus::Completed) {
                break;
            }

            let _ = tx_clone.send(StreamEvent::EvaluateStart);

            let (latest_frame_b64, capture_instant) = {
                let f_arc = { frame_rx_clone.borrow().clone() };
                if let Some(timestamped_frame) = f_arc {
                    let instant = timestamped_frame.captured_at;
                    let b64 = process_and_encode_frame(timestamped_frame.buffer).await;
                    (b64, instant)
                } else {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
            };

            {
                let mut session = eval_session_arc.lock().await;
                let current_plan_text =
                    serde_json::to_string(&plan).unwrap_or_else(|_| "[]".to_string());
                session.history.truncate(2);

                session.history.push(Message::User {
                    content: Content::Blocks(vec![
                        ContentBlock::Text {
                            text: format!("Current plan: {}", current_plan_text),
                        },
                        ContentBlock::Text {
                            text: "Current Image".to_string(),
                        },
                        ContentBlock::ImageUrl {
                            image_url: ImageUrlPayload {
                                url: format!("data:image/jpeg;base64,{}", latest_frame_b64),
                                detail: None,
                            },
                        },
                    ]),
                    name: None,
                });
            }

            let _ = client_clone
                .run_agent_loop(
                    eval_session_arc.clone(),
                    tx_clone.clone(),
                    Some(ReasoningEffort::None),
                )
                .await;

            let total_latency = capture_instant.elapsed();
            let _ = tx_clone.send(StreamEvent::Latency(total_latency));
        }
    });
}

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

            append_chat_display(app, Role::User, raw_prompt.clone());
            append_chat_display(app, Role::Assistant, String::new());

            let action = UserAction::parse(&raw_prompt, &app.skill_registry);
            handle_submission(app, action);
        }
        _ => {
            if !app.is_generating {
                app.input_textarea.input(key_event);
            }
        }
    }
}

pub fn handle_stream_event<P: AppProvider>(app: &mut App<P>, event: StreamEvent) {
    let current_kind = match &event {
        StreamEvent::Reasoning(_) => EventKind::Reasoning,
        StreamEvent::Content(_) => EventKind::Content,
        StreamEvent::ToolExecution { .. } => EventKind::Tool,
        _ => EventKind::Other,
    };

    if current_kind != EventKind::Other && app.last_event_kind != current_kind {
        commit_markdown_buffers(app);
    }

    if current_kind != EventKind::Other {
        app.last_event_kind = current_kind;
    }

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
                format!("\nExecuting {} with {}\n", name, args),
            );
        }
        StreamEvent::ToolError { name, args, error } => {
            append_error(
                app,
                &format!("\nFailed {} with {}, {}\n", name, args, error),
            );
        }
        StreamEvent::PlanUpdated(new_plan) => {
            app.plan = new_plan;
        }
        StreamEvent::EvaluateStart => {
            append_chat_display(app, Role::Info, format!("\nEvaluating Plan Progress"));
        }
        StreamEvent::Usage(usage) => {
            app.last_usage = usage;
        }
        StreamEvent::Metrics(metrics) => {
            app.last_metrics = Some(metrics);
        }
        StreamEvent::Latency(duration) => {
            app.last_latency = Some(duration);
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
