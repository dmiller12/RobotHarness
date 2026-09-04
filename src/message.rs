use serde::{Deserialize, Serialize};

use crate::api::ToolCall;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ImageUrl {
        image_url: ImageUrlPayload,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageUrlPayload {
    /// Format: "data:image/jpeg;base64,{base64_string}" or a public URL
    pub url: String,
    /// Specifies vision token processing: "low", "high", or "auto"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    System {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    User {
        content: Content, // Uses your untagged Text/Blocks enum
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    Assistant {
        // Assistant content can be omitted if it only outputs tool calls
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>, 
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>, // Or your custom ToolCall struct
    },
    Tool {
        content: String,
        tool_call_id: String,
    },
}
