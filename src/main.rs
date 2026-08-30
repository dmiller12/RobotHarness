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
}

#[derive(Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

pub struct Session {
    client: Client,
    model: String,
    endpoint: String,
    pub history: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delta {
    pub content: Option<String>,
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
        let new_message = Message {
            role: Role::User,
            content: user_input.to_string(),
        };
        self.history.push(new_message);

        let request = Request {
            model: self.model.clone(),
            messages: self.history.clone(),
            temperature: Some(0.7),
            stream: Some(true),
        };

        let response = self
            .client
            .post(&self.endpoint)
            .json(&request)
            .send()
            .await?;

        let mut stream = response.bytes_stream();
        let mut full_text = String::new();
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            let bytes = chunk_result?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(newline_idx) = buffer.find('\n') {
                let line = buffer[..newline_idx].to_string();
                buffer.drain(..=newline_idx); // Remove the processed line from the buffer

                let trimmed_line = line.trim();
                if trimmed_line.is_empty() {
                    continue;
                }

                // Safely strip prefix without unwrap()
                if let Some(json_data) = trimmed_line.strip_prefix("data: ") {
                    let json_data = json_data.trim();

                    if json_data == "[DONE]" {
                        break;
                    }

                    // Attempt to parse the valid JSON payload
                    if let Ok(stream_res) = serde_json::from_str::<StreamResponse>(json_data) {
                        if let Some(choice) = stream_res.choices.into_iter().next() {
                            if let Some(fragment) = choice.delta.content {
                                print!("{}", fragment);
                                io::stdout().flush()?;
                                full_text.push_str(&fragment);
                            }
                        }
                    } else {
                        eprintln!("\n[Warning: Failed to parse JSON chunk: {}]", json_data);
                    }
                }
            }
        }
        println!();

        self.history.push(Message {
            role: Role::Assistant,
            content: full_text.clone(),
        });

        Ok(full_text)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::new(
        "qwen3.5:4b-mlx",
        "http://localhost:11434/v1/chat/completions",
        "You are a helpful assistant.",
    );

    let prompt1 = "Explain Newtons Laws";
    println!("User: {}", prompt1);

    let reply1 = session.chat(prompt1).await?;
    println!("Assistant: {}\n", reply1);

    // 3. Execute a second turn to prove history is maintained
    let prompt2 = "Which of those applies most directly to rocket propulsion?";
    println!("User: {}", prompt2);

    let reply2 = session.chat(prompt2).await?;
    println!("Assistant: {}", reply2);

    Ok(())
}
