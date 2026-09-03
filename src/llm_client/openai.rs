use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    api::{Message, StreamResponse, Tool, ToolCall, ToolCallFunction},
    session::StreamEvent,
};

use super::provider::{GenerationResult, LlmProvider};

pub struct OpenAiProvider {
    client: Client,
    model: String,
    endpoint: String,
}

impl OpenAiProvider {
    pub fn new(model: &str, endpoint: &str) -> Self {
        Self {
            client: Client::new(),
            model: model.to_string(),
            endpoint: endpoint.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct OpenAiRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
}
#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn stream_completion(
        &self,
        messages: &[Message],
        tools: &[Tool],
        tx: UnboundedSender<StreamEvent>,
    ) -> Result<GenerationResult, Box<dyn std::error::Error>> {
        let request = OpenAiRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            temperature: Some(0.7),
            stream: Some(true),
            reasoning_effort: Some("high".to_string()),
            tools: if tools.is_empty() {
                None
            } else {
                Some(tools.to_vec())
            },
        };

        let request_builder = self.client.post(&self.endpoint).json(&request);
        let mut event_source = EventSource::new(request_builder)?;

        let mut final_content = String::new();
        // Use a BTreeMap to accumulate parallel tool call chunks by their index
        let mut tool_calls_map: BTreeMap<usize, ToolCall> = BTreeMap::new();

        while let Some(event_result) = event_source.next().await {
            match event_result {
                Ok(Event::Open) => continue,
                Ok(Event::Message(message)) => {
                    if message.data == "[DONE]" {
                        break;
                    }

                    if let Ok(stream_res) = serde_json::from_str::<StreamResponse>(&message.data) {
                        if let Some(choice) = stream_res.choices.into_iter().next() {
                            if let Some(reasoning) = choice.delta.reasoning {
                                if !reasoning.is_empty() {
                                    let _ = tx.send(StreamEvent::Reasoning(reasoning));
                                }
                            }

                            if let Some(content) = choice.delta.content {
                                if !content.is_empty() {
                                    final_content.push_str(&content);
                                    let _ = tx.send(StreamEvent::Content(content.clone()));
                                }
                            }

                            if let Some(tc_deltas) = choice.delta.tool_calls {
                                for tc_delta in tc_deltas {
                                    let idx = tc_delta.index;
                                    let idx_usize = idx as usize;
                                    let entry =
                                        tool_calls_map.entry(idx_usize).or_insert_with(|| ToolCall {
                                            index: idx,
                                            id: tc_delta.id.clone(),
                                            r#type: Some("function".to_string()),
                                            function: ToolCallFunction {
                                                name: tc_delta.function.name.clone(),
                                                arguments: Some(String::new()),
                                            },
                                        });

                                    if let Some(args_chunk) = tc_delta.function.arguments {
                                        if let Some(existing_args) = &mut entry.function.arguments {
                                            existing_args.push_str(&args_chunk);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    let _ = tx.send(StreamEvent::Error(err.to_string()));
                    event_source.close();
                    break;
                }
            }
        }

        let final_tool_calls: Vec<ToolCall> = tool_calls_map.into_values().collect();

        Ok(GenerationResult {
            content: final_content,
            tool_calls: final_tool_calls,
        })
    }

}#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use crate::api::Role;

    #[tokio::test]
    async fn test_openai_stream() {
        let provider = OpenAiProvider::new("qwen3.5:4b-mlx", "http://localhost:11434/v1/chat/completions");

        let messages = vec![Message {
            role: Role::User,
            content: Some("Count from 1 to 3.".to_string()),
            tool_calls: None,
            tool_call_id: None,
        }];

        let (tx, mut rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                println!("Stream Event: {:?}", event);
            }
        });

        let result = provider.stream_completion(&messages, &[], tx).await;
        
        println!("Final Output: {:?}", result);
    }
}
