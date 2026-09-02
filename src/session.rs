use futures_util::StreamExt;
use reqwest::Client;

use crate::api::{
    FunctionDeclaration, Message, Request, Role, StreamResponse, Tool, ToolCall, ToolCallFunction,
};
use crate::tools::execute_tool;

use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Reasoning(String),
    Content(String),
    ToolExecution { name: String, args: String },
    Error(String),
    PlanUpdated(Vec<Task>),
    Done,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Task {
    pub description: String,
    pub status: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SessionData {
    history: Vec<Message>,
    plan: Vec<Task>,
}

#[derive(Debug)]
pub struct Session {
    client: Client,
    model: String,
    endpoint: String,
    pub plan: Vec<Task>,
    pub history: Vec<Message>,
}

impl Session {
    pub fn new(model: &str, endpoint: &str, system_prompt: &str) -> Self {
        Self {
            client: Client::new(),
            model: model.to_string(),
            endpoint: endpoint.to_string(),
            plan: Vec::new(),
            history: vec![Message {
                role: Role::System,
                content: Some(system_prompt.to_string()),
                tool_calls: None,
                tool_call_id: None,
            }],
        }
    }

    fn get_session_file() -> Result<PathBuf, String> {
        let mut path = dirs::home_dir().ok_or("Could not resolve home directory")?;
        path.push(".harness");

        if !path.exists() {
            fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        }
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;

        let folder_name = cwd
            .file_name()
            .and_then(|os_str| os_str.to_str())
            .ok_or("Invalid Unicode in directory name")?;

        path.push(format!("{}.json", folder_name));
        Ok(path)
    }

    pub fn load_state(&mut self) -> Result<(), String> {
        let path = Self::get_session_file()?;
        if path.exists() {
            let json = fs::read_to_string(path).map_err(|e| e.to_string())?;

            // Try parsing the new wrapper format first
            if let Ok(data) = serde_json::from_str::<SessionData>(&json) {
                self.history = data.history;
                self.plan = data.plan;
            }
            // Fallback for older saves that were just a raw Vec<Message>
            else if let Ok(history) = serde_json::from_str::<Vec<Message>>(&json) {
                self.history = history;
            } else {
                return Err("Failed to parse session file".to_string());
            }
        }
        Ok(())
    }

    pub fn save_state(&self) -> Result<(), String> {
        let path = Self::get_session_file()?;
        let data = SessionData {
            history: self.history.clone(),
            plan: self.plan.clone(),
        };
        let json = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn dump_to_stdout(&self) {
        for msg in &self.history {
            let role_name = match msg.role {
                Role::System => "System",
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::Tool => "Tool",
            };

            if let Some(text) = &msg.content {
                println!("\x1b[1m[{}]\x1b[0m: {}", role_name, text.trim());
            } else if let Some(tools) = &msg.tool_calls {
                for tc in tools {
                    if let Some(name) = &tc.function.name {
                        println!("\x1b[1m[{}]\x1b[0m: <Executed Tool: {}>", role_name, name);
                    }
                }
            }
        }
        println!();
    }

    pub async fn chat(
        &mut self,
        user_input: &str,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        if !user_input.is_empty() {
            self.history.push(Message {
                role: Role::User,
                content: Some(user_input.to_string()),
                tool_calls: None,
                tool_call_id: None,
            });
        }
        let plan_tool = Tool {
            r#type: "function".to_string(),
            function: FunctionDeclaration {
                name: "update_plan".to_string(),
                description: "Update the current execution plan and task statuses.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "tasks": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "description": { "type": "string" },
                                    "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] }
                                },
                                "required": ["description", "status"]
                            }
                        }
                    },
                    "required": ["tasks"]
                }),
            },
        };
        let tools = vec![
            Tool {
                r#type: "function".to_string(),
                function: FunctionDeclaration {
                    name: "read_file".to_string(),
                    description: "Read the contents of a file from the local filesystem."
                        .to_string(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "The relative or absolute path to the file."
                            }
                        },
                        "required": ["path"]
                    }),
                },
            },
            plan_tool,
        ];

        let request = Request {
            model: self.model.clone(),
            messages: self.history.clone(),
            temperature: Some(0.7),
            stream: Some(true),
            reasoning_effort: Some("high".to_string()),
            tools: Some(tools),
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

        let mut active_tool_id = String::new();
        let mut active_tool_name = String::new();
        let mut active_tool_args = String::new();

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
                                if !reasoning.is_empty() {
                                    let _ = tx.send(StreamEvent::Reasoning(reasoning));
                                }
                            }

                            // 2. Handle standard content stream (reset color)
                            if let Some(fragment) = choice.delta.content {
                                if !fragment.is_empty() {
                                    let _ = tx.send(StreamEvent::Content(fragment.clone()));
                                    assistant_response.push_str(&fragment);
                                }
                            }

                            if let Some(tool_calls) = choice.delta.tool_calls {
                                for tc in tool_calls {
                                    if let Some(id) = tc.id {
                                        active_tool_id = id;
                                    }

                                    if let Some(name) = tc.function.name {
                                        active_tool_name = name;
                                    }

                                    if let Some(args) = tc.function.arguments {
                                        active_tool_args.push_str(&args);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if !active_tool_name.is_empty() {
            let _ = tx.send(StreamEvent::ToolExecution {
                name: active_tool_name.clone(),
                args: active_tool_args.clone(),
            });

            self.history.push(Message {
                role: Role::Assistant,
                content: if assistant_response.is_empty() {
                    None
                } else {
                    Some(assistant_response.clone())
                },
                tool_calls: Some(vec![ToolCall {
                    index: 0,
                    id: Some(active_tool_id.clone()),
                    r#type: Some("function".to_string()),
                    function: ToolCallFunction {
                        name: Some(active_tool_name.clone()),
                        arguments: Some(active_tool_args.clone()),
                    },
                }]),
                tool_call_id: None,
            });

            let tool_output = execute_tool(&active_tool_name, &active_tool_args, self);

            if active_tool_name == "update_plan" {
                let _ = tx.send(StreamEvent::PlanUpdated(self.plan.clone()));
            }

            self.history.push(Message {
                role: Role::Tool,
                content: Some(tool_output),
                tool_calls: None,
                tool_call_id: Some(active_tool_id),
            });

            return Box::pin(self.chat("", tx.clone())).await;
        }

        self.history.push(Message {
            role: Role::Assistant,
            content: Some(assistant_response.clone()),
            tool_calls: None,
            tool_call_id: None,
        });

        Ok(assistant_response)
    }
}
