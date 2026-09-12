pub mod openai;
pub mod provider;

use crate::llm_client::openai::ReasoningEffort;
use crate::message::Message;
use crate::session::{SessionData, StreamEvent};
use crate::tools::ToolRegistry;
use provider::LlmProvider;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct LlmClient<P: LlmProvider> {
    provider: P,
    tool_registry: ToolRegistry,
}

impl<P: LlmProvider> LlmClient<P> {
    pub fn new(provider: P, tool_registry: ToolRegistry) -> Self {
        Self {
            provider,
            tool_registry,
        }
    }

    pub async fn run_agent_loop(
        &self,
        session_arc: Arc<Mutex<SessionData>>,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        reasoning_effort: Option<ReasoningEffort>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            let messages = {
                let session = session_arc.lock().await;
                session.history.clone()
            };
            let tools = self.tool_registry.get_api_tools();

            let result = self
                .provider
                .stream_completion(&messages, &tools, tx.clone(), reasoning_effort)
                .await?;

            {
                let mut session = session_arc.lock().await;
                session.history.push(Message::Assistant {
                    content: if result.content.is_empty() {
                        None
                    } else {
                        Some(result.content.clone())
                    },
                    reasoning: if  result.tool_calls.is_empty() || result.reasoning.is_empty() {
                        None
                    } else {
                        Some(result.reasoning.clone())
                    },
                    tool_calls: if result.tool_calls.is_empty() {
                        None
                    } else {
                        Some(result.tool_calls.clone())
                    },
                    name: None,
                });
            }

            if result.tool_calls.is_empty() {
                break;
            }
            let mut requires_roundtrip = false;

            for tool_call in result.tool_calls {
                let name = tool_call.function.name.unwrap_or_default();
                let args = tool_call.function.arguments.unwrap_or_default();
                let id = tool_call.id.unwrap_or_default();

                let _ = tx.send(StreamEvent::ToolExecution {
                    name: name.clone(),
                    args: args.clone(),
                });
                let tool = self
                    .tool_registry
                    .get_tool(&name)
                    .ok_or_else(|| format!("Tool '{}' not found in registry", name))?;

                let tool_output = match self
                    .tool_registry
                    .execute_tool(&name, &args, tx.clone())
                    .await
                {
                    Ok(success_msg) => {
                        if tool.roundtrip_on_success() {
                            requires_roundtrip = true;
                        }
                        success_msg
                    }
                    Err(error_msg) => {
                        let _ = tx.send(StreamEvent::ToolError {
                            name: name.clone(),
                            args: args.clone(),
                            error: error_msg.clone(),
                        });
                        requires_roundtrip = true;
                        error_msg
                    } // Feed errors back so the LLM can self-correct
                };

                let mut session = session_arc.lock().await;
                session.history.push(Message::Tool {
                    content: tool_output,
                    tool_call_id: id,
                });
                session.save_state();
            }
            if !requires_roundtrip {
                break;
            }
        }

        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_client::openai::OpenAiProvider;
    use crate::message::{Content, ContentBlock, ImageUrlPayload};
    use base64::prelude::*;
    use std::fs;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_run_agent_loop() {
        let provider = OpenAiProvider::new(
            "qwen3.5:4b-mlx",
            "http://localhost:11434/v1/chat/completions",
        );

        let client = LlmClient::new(provider, ToolRegistry::new());

        let mut session_data = SessionData::new("You are a concise test assistant.");
        session_data.history.push(Message::User {
            content: Content::Text("Respond with exactly: 'Agent loop functional.'".to_string()),
            name: None,
        });

        let session_arc = Arc::new(Mutex::new(session_data));

        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                println!("UI Event: {:?}", event);
            }
        });

        let result = client.run_agent_loop(session_arc.clone(), tx).await;

        assert!(
            result.is_ok(),
            "Agent loop returned an error: {:?}",
            result.err()
        );

        let final_session = session_arc.lock().await;
        println!("\n--- Final Session History ---");
        for msg in &final_session.history {
            println!("[{:?}]", msg);
        }

        assert_eq!(final_session.history.len(), 3);
    }

    #[tokio::test]
    async fn test_image() {
        let test_image_path = "test_image.jpg";
        let image_bytes = fs::read(test_image_path)
            .unwrap_or_else(|_| panic!("Failed to read {}", test_image_path));

        let base64_string = BASE64_STANDARD.encode(&image_bytes);
        let provider = OpenAiProvider::new(
            "qwen3.5:4b-mlx",
            "http://localhost:11434/v1/chat/completions",
        );

        let client = LlmClient::new(provider, ToolRegistry::new());

        let mut session_data = SessionData::new("You are a concise assistant.");
        session_data.history.push(Message::User {
            content: Content::Blocks(vec![
                ContentBlock::Text {
                    text: "What is in the image?".to_string(),
                },
                ContentBlock::ImageUrl {
                    image_url: ImageUrlPayload {
                        url: format!("data:image/jpeg;base64,{}", base64_string),
                        detail: None,
                    },
                },
            ]),
            name: None,
        });

        let session_arc = Arc::new(Mutex::new(session_data));

        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                println!("UI Event: {:?}", event);
            }
        });

        let result = client.run_agent_loop(session_arc.clone(), tx).await;

        assert!(
            result.is_ok(),
            "Agent loop returned an error: {:?}",
            result.err()
        );

        let final_session = session_arc.lock().await;
        println!("\n--- Final Session History ---");
        for msg in &final_session.history {
            println!("[{:?}]", msg);
        }

        assert_eq!(final_session.history.len(), 3);
    }
}
