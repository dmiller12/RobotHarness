use tokio::sync::mpsc::UnboundedSender;
use async_trait::async_trait;

use crate::{api::{Tool, ToolCall, Usage}, llm_client::openai::ReasoningEffort, session::StreamEvent};
use crate::message::Message;


pub trait AppProvider: LlmProvider + Send + Sync + 'static {}
impl<T: LlmProvider + Send + Sync + 'static> AppProvider for T {}

#[derive(Debug)]
pub struct GenerationResult {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Default)]
pub struct GenerationMetrics {
    pub ttft_ms: u128,
    pub generation_ms: f64,
    pub tps: f64,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream_completion(
        &self, 
        messages: &[Message],
        tools: &[Tool],
        tx: UnboundedSender<StreamEvent>,
        reasoning_effort: Option<ReasoningEffort>,
    ) -> Result<GenerationResult, Box<dyn std::error::Error>>;
}
