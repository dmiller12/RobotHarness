use crate::api::Role;
use crate::llm_client::provider::AppProvider;
use crate::message::Message;
use crate::message::{Content, ContentBlock, ImageUrlPayload};
use crate::ui::{append_chat_display, append_error, commit_markdown_buffers, reset_textarea};
use crate::{app::App, session::StreamEvent};
use image;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};
use nokhwa::pixel_format::RgbFormat;

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
            // TODO: Add user input parsing into separate app commands
            let (is_frame_req, actual_prompt) =
                if let Some(stripped) = raw_prompt.strip_prefix("/frame") {
                    (true, stripped.trim().to_string())
                } else {
                    (false, raw_prompt.to_string())
                };

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
                let mut message_blocks = vec![ContentBlock::Text {
                    text: actual_prompt,
                }];

                // 3. Only process the camera frame if requested
                if is_frame_req {
                    let frame_arc = {
                        let rx_lock = frame_rx_clone.borrow();
                        rx_lock.clone()
                    };

                    if let Some(frame) = frame_arc {
                        let base64_image = tokio::task::spawn_blocking(move || {
                            let decoded = frame
                                .decode_image::<RgbFormat>()
                                .expect("Failed to decode RGB");
                            let img = image::DynamicImage::ImageRgb8(decoded);

                            let resized =
                                img.resize_exact(512, 512, image::imageops::FilterType::Nearest);

                            let mut jpeg_bytes: Vec<u8> = Vec::new();
                            let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);
                            resized
                                .write_to(&mut cursor, image::ImageFormat::Jpeg)
                                .expect("Failed to encode JPEG");

                            use base64::prelude::*;
                            BASE64_STANDARD.encode(&jpeg_bytes)
                        })
                        .await
                        .expect("Image processing thread panicked");

                        // 4. Append the image payload to the blocks array
                        message_blocks.push(ContentBlock::ImageUrl {
                            image_url: ImageUrlPayload {
                                url: format!("data:image/jpeg;base64,{}", base64_image),
                                detail: None,
                            },
                        });
                    }
                }

                // 5. Push the constructed message to history
                {
                    let mut session = session_clone.lock().await;
                    session.history.push(Message::User {
                        content: Content::Blocks(message_blocks),
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
