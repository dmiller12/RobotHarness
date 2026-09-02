mod api;
mod app;
mod session;
mod tools;
mod ui;
mod events;

use std::io::{self};

use crate::app::App;
use crate::session::Session;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture,
};
use crossterm::execute;

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut session = Session::new(
        "qwen3.5:4b-mlx",
        "http://localhost:11434/v1/chat/completions",
        "You are a helpful assistant.",
    );

    let _ = session.load_state();
    let mut terminal = ratatui::init();
    execute!(io::stdout(), EnableMouseCapture)?;
    let mut app = App::new(session);
    let _app_result = app.run(&mut terminal).await;
    execute!(io::stdout(), DisableMouseCapture)?;
    ratatui::restore();

    Ok(())
}
