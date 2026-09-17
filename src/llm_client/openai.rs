use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

use crate::api::Usage;
use crate::llm_client::provider::GenerationMetrics;
use crate::message::Message;

use crate::{
    api::{StreamResponse, Tool, ToolCall, ToolCallFunction},
    session::StreamEvent,
};

use super::provider::{GenerationResult, LlmProvider};

pub struct OpenAiProvider {
    client: Client,
    model: String,
    endpoint: String,
    api_key: Option<String>,
}

impl OpenAiProvider {
    pub fn new(model: &str, endpoint: &str, api_key: Option<String>) -> Self {
        Self {
            client: Client::new(),
            model: model.to_string(),
            endpoint: endpoint.to_string(),
            api_key: api_key,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StreamOptions {
    pub include_usage: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Low,
    Medium,
    High,
}

#[derive(Serialize, Deserialize)]
pub struct OpenAiRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn stream_completion(
        &self,
        messages: &[Message],
        tools: &[Tool],
        tx: UnboundedSender<StreamEvent>,
        reasoning_effort: Option<ReasoningEffort>,
    ) -> Result<GenerationResult, Box<dyn std::error::Error>> {
        let request = OpenAiRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            temperature: Some(0.6),
            // TODO, may need to pass these seperately
            presence_penalty: Some(0.0),
            frequency_penalty: Some(1.0),
            // presence_penalty: None,
            // frequency_penalty: None,
            top_p: Some(0.95),
            stream: Some(true),
            reasoning_effort: reasoning_effort,
            tools: if tools.is_empty() {
                None
            } else {
                Some(tools.to_vec())
            },
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
        };

        let mut request_builder = self.client.post(&self.endpoint).json(&request);
        if let Some(key) = &self.api_key {
            request_builder = request_builder.bearer_auth(key);
        }
        let mut event_source = EventSource::new(request_builder)?;

        let mut final_reasoning = String::new();
        let mut final_content = String::new();
        let mut tool_calls_map: BTreeMap<usize, ToolCall> = BTreeMap::new();

        let mut final_usage: Option<Usage> = None;

        let request_start = Instant::now();
        let mut first_token_time: Option<Instant> = None;
        while let Some(event_result) = event_source.next().await {
            match event_result {
                Ok(Event::Open) => continue,
                Ok(Event::Message(message)) => {
                    if message.data == "[DONE]" {
                        break;
                    }

                    match serde_json::from_str::<StreamResponse>(&message.data) {
                        Ok(stream_res) => {
                            if let Some(choice) = stream_res.choices.into_iter().next() {
                                if let Some(reasoning) = choice.delta.reasoning {
                                    if !reasoning.is_empty() {
                                        if first_token_time.is_none() {
                                            first_token_time = Some(Instant::now());
                                        }
                                        final_reasoning.push_str(&reasoning);
                                        let _ = tx.send(StreamEvent::Reasoning(reasoning));
                                    }
                                }

                                if let Some(content) = choice.delta.content {
                                    if !content.is_empty() {
                                        if first_token_time.is_none() {
                                            first_token_time = Some(Instant::now());
                                        }
                                        final_content.push_str(&content);
                                        let _ = tx.send(StreamEvent::Content(content.clone()));
                                    }
                                }

                                if let Some(tc_deltas) = choice.delta.tool_calls {
                                    if first_token_time.is_none() {
                                        first_token_time = Some(Instant::now());
                                    }

                                    for tc_delta in tc_deltas {
                                        let idx = tc_delta.index.unwrap_or(0);
                                        let idx_usize = idx as usize;
                                        let entry =
                                            tool_calls_map.entry(idx_usize).or_insert_with(|| {
                                                ToolCall {
                                                    index: Some(idx),
                                                    id: tc_delta.id.clone(),
                                                    r#type: Some("function".to_string()),
                                                    function: ToolCallFunction {
                                                        name: tc_delta.function.name.clone(),
                                                        arguments: Some(String::new()),
                                                    },
                                                }
                                            });

                                        if let Some(args_chunk) = tc_delta.function.arguments {
                                            if let Some(existing_args) =
                                                &mut entry.function.arguments
                                            {
                                                existing_args.push_str(&args_chunk);
                                            }
                                        }
                                    }
                                }
                            }

                            if let Some(usage) = stream_res.usage {
                                let mut metrics = GenerationMetrics::default();
                                if let Some(ft_time) = first_token_time {
                                    metrics.ttft_ms =
                                        ft_time.duration_since(request_start).as_millis();
                                    let gen_duration = ft_time.elapsed().as_secs_f64();
                                    metrics.generation_ms = gen_duration * 1000.0;

                                    if gen_duration > 0.0 {
                                        metrics.tps = usage.completion_tokens as f64 / gen_duration;
                                    }
                                }
                                final_usage = Some(usage);
                                let _ = tx.send(StreamEvent::Metrics(metrics));
                                let _ = tx.send(StreamEvent::Usage(usage.clone()));
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "Deserialization Error on chunk: {} | Data: {}",
                                e, message.data
                            );
                        }
                    }
                }
                Err(err) => {
                    let error_msg = match err {
                        reqwest_eventsource::Error::InvalidStatusCode(status, response) => {
                            let body = response.text().await.unwrap_or_default();
                            format!("API Error {}: {}", status, body)
                        }
                        reqwest_eventsource::Error::InvalidContentType(header, response) => {
                            let body = response.text().await.unwrap_or_default();
                            format!("Invalid Content-Type {:?}: {}", header, body)
                        }
                        other_err => other_err.to_string(),
                    };

                    let _ = tx.send(StreamEvent::Error(error_msg));
                    event_source.close();
                    break;
                }
            }
        }

        let final_tool_calls: Vec<ToolCall> = tool_calls_map.into_values().collect();

        Ok(GenerationResult {
            reasoning: final_reasoning,
            content: final_content,
            tool_calls: final_tool_calls,
            usage: final_usage,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Content;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_openai_stream() {
        let provider = OpenAiProvider::new(
            "qwen3.5:4b-mlx",
            "http://localhost:11434/v1/chat/completions",
            None,
        );

        let messages = vec![Message::User {
            content: Content::Text("Count from 1 to 3.".to_string()),
            name: None,
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
