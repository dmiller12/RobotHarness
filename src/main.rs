mod api;
mod app;
mod events;
mod llm_client;
mod session;
mod tools;
mod ui;

use std::io::{self};

use crate::app::App;
use crate::llm_client::LlmClient;
use crate::llm_client::openai::OpenAiProvider;
use crate::session::SessionData;
use crate::tools::ToolRegistry;
use crate::tools::read_file::ReadFileTool;
use crate::tools::update_plan::UpdatePlanTool;
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

    let session_data = SessionData::load().expect("Failed to load session state");
    let session = Arc::new(Mutex::new(session_data));

    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(Box::new(ReadFileTool));
    tool_registry.register(Box::new(UpdatePlanTool { session: session.clone() }));

    let client = LlmClient::new(provider, tool_registry);

    let mut terminal = ratatui::init();
    execute!(io::stdout(), EnableMouseCapture)?;
    let mut app = App::new(session, client);
    let _app_result = app.run(&mut terminal).await;
    execute!(io::stdout(), DisableMouseCapture)?;
    ratatui::restore();

    Ok(())
}
