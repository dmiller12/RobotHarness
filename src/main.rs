use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delta {
    pub content: Option<String>,
    #[serde(alias = "reasoning_content")]
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChoice {
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamResponse {
    pub choices: Vec<StreamChoice>,
}

pub struct Session {
    client: Client,
    model: String,
    endpoint: String,
    pub history: Vec<Message>,
}

impl Session {
    pub fn new(model: &str, endpoint: &str, system_prompt: &str) -> Self {
        Self {
            client: Client::new(),
            model: model.to_string(),
            endpoint: endpoint.to_string(),
            history: vec![Message {
                role: Role::System,
                content: system_prompt.to_string(),
            }],
        }
    }

    pub async fn chat(&mut self, user_input: &str) -> Result<String, Box<dyn std::error::Error>> {
        self.history.push(Message {
            role: Role::User,
            content: user_input.to_string(),
        });

        let request = Request {
            model: self.model.clone(),
            messages: self.history.clone(),
            temperature: Some(0.7),
            stream: Some(true),
            reasoning_effort: Some("high".to_string()),
        };

        let response = self
            .client
            .post(&self.endpoint)
            .json(&request)
            .send()
            .await?;

        let mut stream = response.bytes_stream();
        let mut assistant_response = String::new();
        let mut network_buffer = String::new();
        let mut is_reasoning = false;

        while let Some(chunk_result) = stream.next().await {
            let bytes = chunk_result?;
            network_buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(newline_idx) = network_buffer.find('\n') {
                let line = network_buffer[..newline_idx].to_string();
                network_buffer.drain(..=newline_idx);

                let trimmed_line = line.trim();
                if trimmed_line.is_empty() {
                    continue;
                }

                if let Some(json_data) = trimmed_line.strip_prefix("data: ") {
                    let json_data = json_data.trim();
                    if json_data == "[DONE]" {
                        break;
                    }

                    if let Ok(stream_res) = serde_json::from_str::<StreamResponse>(json_data) {
                        if let Some(choice) = stream_res.choices.into_iter().next() {
                            // 1. Handle native reasoning stream (dim gray)
                            if let Some(reasoning) = choice.delta.reasoning {
                                if !is_reasoning {
                                    print!("\x1b[90m");
                                    is_reasoning = true;
                                }
                                print!("{}", reasoning);
                                io::stdout().flush()?;
                            }

                            // 2. Handle standard content stream (reset color)
                            if let Some(fragment) = choice.delta.content {
                                if is_reasoning {
                                    print!("\x1b[0m");
                                    is_reasoning = false;
                                }
                                print!("{}", fragment);
                                io::stdout().flush()?;
                                assistant_response.push_str(&fragment);
                            }
                        }
                    }
                }
            }
        }

        print!("\x1b[0m\n");
        io::stdout().flush()?;

        self.history.push(Message {
            role: Role::Assistant,
            content: assistant_response.clone(),
        });

        Ok(assistant_response)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::new(
        "qwen3.5:4b-mlx",
        "http://localhost:11434/v1/chat/completions",
        "You are a helpful assistant.",
    );

    println!("Session started. Type 'quit' to exit.\n");

    loop {
        print!("User: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();

        if trimmed == "quit" || trimmed.is_empty() {
            break;
        }

        print!("Assistant: ");
        io::stdout().flush()?;

        session.chat(trimmed).await?;
        println!();
    }

    Ok(())
}
