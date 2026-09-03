mod api;
mod app;
mod events;
mod llm_client;
mod session;
mod tools;
mod ui;

use std::io::{self};

use crate::api::{FunctionDeclaration, Tool};
use crate::app::App;
use crate::llm_client::LlmClient;
use crate::llm_client::openai::OpenAiProvider;
use crate::session::SessionData;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> io::Result<()> {
    let provider = OpenAiProvider::new(
        "qwen3.5:4b-mlx",
        "http://localhost:11434/v1/chat/completions",
    );

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
                description: "Read the contents of a file from the local filesystem.".to_string(),
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

    let session_data = SessionData::load().expect("Failed to load session state");
    let session = Arc::new(Mutex::new(session_data));

    let client = LlmClient::new(provider, tools);

    let mut terminal = ratatui::init();
    execute!(io::stdout(), EnableMouseCapture)?;
    let mut app = App::new(session, client);
    let _app_result = app.run(&mut terminal).await;
    execute!(io::stdout(), DisableMouseCapture)?;
    ratatui::restore();

    Ok(())
}
