use tokio::sync::mpsc::UnboundedSender;
use async_trait::async_trait;

use crate::{api::{Tool, ToolCall}, session::StreamEvent};
use crate::message::Message;


pub trait AppProvider: LlmProvider + Send + Sync + 'static {}
impl<T: LlmProvider + Send + Sync + 'static> AppProvider for T {}

#[derive(Debug)]
pub struct GenerationResult {
    pub content: String,
    pub tool_calls: Vec<ToolCall>
}
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream_completion(
        &self, 
        messages: &[Message],
        tools: &[Tool],
        tx: UnboundedSender<StreamEvent>,
    ) -> Result<GenerationResult, Box<dyn std::error::Error>>;
}
