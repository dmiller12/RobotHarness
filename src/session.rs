use crate::api::{Message, Role};
use schemars::{JsonSchema};

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct Task {
    /// A clear, concise summary of the task to be performed.
    pub description: String,
    /// The current execution status of the task.
    pub status: TaskStatus,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SessionData {
    pub history: Vec<Message>,
    pub plan: Vec<Task>,
}

impl SessionData {
    pub fn new(system_prompt: &str) -> Self {
        Self {
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

    pub fn load() -> Result<Self, String> {
        Self::load_from_path(Self::get_session_file()?)
    }

    pub fn load_from_path(path: PathBuf) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!("Session file not found: {}", path.display()));
        }

        let json = fs::read_to_string(&path).map_err(|e| e.to_string())?;

        if let Ok(data) = serde_json::from_str::<SessionData>(&json) {
            return Ok(data);
        }

        if let Ok(history) = serde_json::from_str::<Vec<Message>>(&json) {
            return Ok(Self {
                history,
                plan: Vec::new(),
            });
        }

        Err("Failed to parse session file".to_string())
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
}
