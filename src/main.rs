mod api;
mod app;
mod events;
mod llm_client;
mod message;
mod session;
mod tools;
mod ui;
mod video;

use std::io::{self};

use crate::app::App;
use crate::llm_client::LlmClient;
use crate::llm_client::openai::OpenAiProvider;
use crate::session::SessionData;
use crate::tools::ToolRegistry;
use crate::tools::read_file::ReadFileTool;
use crate::tools::update_plan::UpdatePlanTool;
use crate::video::run_gui;
use crate::video::start_camera_thread;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::thread;

fn main() -> io::Result<()> {
    let camera_rx = start_camera_thread();
    let agent_camera_rx = camera_rx.clone();

    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        rt.block_on(async move {
            let provider = OpenAiProvider::new(
                "qwen3.5:4b-mlx",
                "http://localhost:11434/v1/chat/completions",
            );

            // let session_data = SessionData::load().expect("Failed to load session state");
            let session_data = SessionData::new("You are are a helpful assistant.");
            let session = Arc::new(Mutex::new(session_data));

            let mut tool_registry = ToolRegistry::new();
            tool_registry.register(Box::new(ReadFileTool));
            tool_registry.register(Box::new(UpdatePlanTool {
                session: session.clone(),
            }));

            let client = LlmClient::new(provider, tool_registry);

            let mut terminal = ratatui::init();
            execute!(io::stdout(), EnableMouseCapture).unwrap();
            let mut app = App::new(session, client, agent_camera_rx);
            let _app_result = app.run(&mut terminal).await;
            execute!(io::stdout(), DisableMouseCapture).unwrap();
            ratatui::restore();
        });
    });

    run_gui(camera_rx);

    Ok(())
}
