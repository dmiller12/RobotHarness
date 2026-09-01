mod api;
mod session;
mod tools;

use std::io::{self, Write};

use crate::session::Session;

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
