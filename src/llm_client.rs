pub mod openai;
pub mod provider;

use crate::api::{Message, Role};
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
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            let messages = {
                let session = session_arc.lock().await;
                session.history.clone()
            };
            let tools = self.tool_registry.get_api_tools();

            let result = self
                .provider
                .stream_completion(&messages, &tools, tx.clone())
                .await?;

            {
                let mut session = session_arc.lock().await;
                session.history.push(Message {
                    role: Role::Assistant,
                    content: if result.content.is_empty() {
                        None
                    } else {
                        Some(result.content.clone())
                    },
                    tool_calls: if result.tool_calls.is_empty() {
                        None
                    } else {
                        Some(result.tool_calls.clone())
                    },
                    tool_call_id: None,
                });
            }

            if result.tool_calls.is_empty() {
                let _ = tx.send(StreamEvent::Done);
                break;
            }

            for tool_call in result.tool_calls {
                let name = tool_call.function.name.unwrap_or_default();
                let args = tool_call.function.arguments.unwrap_or_default();
                let id = tool_call.id.unwrap_or_default();

                let _ = tx.send(StreamEvent::ToolExecution {
                    name: name.clone(),
                    args: args.clone(),
                });

                let tool_output = match self.tool_registry.execute_tool(&name, &args).await {
                    Ok(success_msg) => success_msg,
                    Err(error_msg) => error_msg, // Feed errors back so the LLM can self-correct
                };

                if name == "update_plan" {
                    let updated_plan = {
                        let session = session_arc.lock().await;
                        session.plan.clone()
                    };
                    let _ = tx.send(StreamEvent::PlanUpdated(updated_plan));
                }

                let mut session = session_arc.lock().await;
                session.history.push(Message {
                    role: Role::Tool,
                    content: Some(tool_output),
                    tool_calls: None,
                    tool_call_id: Some(id),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Role;
    use crate::llm_client::openai::OpenAiProvider;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_run_agent_loop() {
        let provider = OpenAiProvider::new(
            "qwen3.5:4b-mlx",
            "http://localhost:11434/v1/chat/completions",
        );

        let client = LlmClient::new(provider, ToolRegistry::new());

        let mut session_data = SessionData::new("You are a concise test assistant.");
        session_data.history.push(Message {
            role: Role::User,
            content: Some("Respond with exactly: 'Agent loop functional.'".to_string()),
            tool_calls: None,
            tool_call_id: None,
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
            println!(
                "[{:?}] {:?}",
                msg.role,
                msg.content.as_deref().unwrap_or("")
            );
        }

        assert_eq!(final_session.history.len(), 3);
    }
}
