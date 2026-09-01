mod api;
mod session;
mod tools;

use std::io::{self};
use std::sync::Arc;

use crate::session::{Session, StreamEvent};
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, 
    MouseEvent, MouseEventKind, EnableMouseCapture, DisableMouseCapture
};
use crossterm::execute;
use ratatui::widgets::Wrap;
use ratatui::{
    DefaultTerminal, Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    symbols::border,
    
    text::{Line, Span},
    widgets::{Block, Paragraph, Widget},
};
use ratatui_textarea::TextArea;
use tokio::sync::{Mutex, mpsc};
use tokio::select;
use futures::StreamExt;

#[derive(Debug)]
pub struct App<'a> {
    pub session: Arc<Mutex<Session>>,
    exit: bool,
    pub is_generating: bool,
    pub was_reasoning: bool,

    pub network_tx: mpsc::UnboundedSender<StreamEvent>,
    pub network_rx: mpsc::UnboundedReceiver<StreamEvent>,

    pub input_textarea: TextArea<'a>,
    pub chat_display: Vec<Line<'static>>,

    pub scroll: u16,
    pub auto_scroll: bool,

}

impl<'a> App<'a> {
    pub fn new(session: Session) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut textarea = TextArea::default();
        textarea.set_block(
            Block::bordered()
                .title(" Prompt ")
                .border_set(border::THICK)
        );
        textarea.set_cursor_line_style(Style::default());
        Self {
            session: Arc::new(Mutex::new(session)),
            exit: false,
            is_generating: false,
            was_reasoning: false,
            input_textarea: textarea,
            network_tx: tx,
            network_rx: rx,
            chat_display: Vec::new(),
            scroll: 0,
            auto_scroll: true
        }
    }

    /// runs the application's main loop until the user quits
    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let mut event_reader = EventStream::new();
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;

            select! {
                // 1. Listen for user keyboard input
                Some(Ok(event)) = event_reader.next() => {
                    self.handle_event(event);
                }

                // 2. Listen for incoming tokens from Ollama
                Some(stream_event) = self.network_rx.recv() => {
                    self.handle_stream_event(stream_event);
                }
            }
        }
        Ok(())
    }
    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event);
            }
            Event::Mouse(mouse_event) => {
                self.handle_mouse_event(mouse_event);
            }
            _ => {}
        }
    }
    fn handle_mouse_event(&mut self, mouse_event: MouseEvent) {
        match mouse_event.kind {
            MouseEventKind::ScrollUp => {
                self.auto_scroll = false;
                // Scroll up 3 lines per tick for smoother trackpad feel
                self.scroll = self.scroll.saturating_sub(3);
            }
            MouseEventKind::ScrollDown => {
                // Scroll down 3 lines per tick
                self.scroll = self.scroll.saturating_add(3);
            }
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(frame.area());

        let history_area = chunks[0];
        
        // 1. Calculate how many visual lines the text will occupy, 
        // accounting for long lines wrapping over the terminal width.
        let inner_width = history_area.width.saturating_sub(2).max(1) as usize;
        let total_lines: u16 = self.chat_display.iter().map(|line| {
            let len = line.width();
            (len.saturating_sub(1) / inner_width) as u16 + 1
        }).sum();

        let history_height = history_area.height.saturating_sub(2);
        let max_scroll = total_lines.saturating_sub(history_height);

        // 2. Adjust the scroll offset based on user state
        if self.auto_scroll {
            self.scroll = max_scroll;
        } else {
            // Prevent scrolling past the bottom
            self.scroll = self.scroll.min(max_scroll);
            // If they scroll all the way to the bottom manually, re-enable auto-scroll
            if self.scroll == max_scroll {
                self.auto_scroll = true; 
            }
        }

        // 3. Render the widgets directly to the frame
        let history_block = Block::bordered()
            .title(Line::from(" Meta-Harness ").centered())
            .border_set(border::THICK);
            
        frame.render_widget(
            Paragraph::new(self.chat_display.clone())
                .block(history_block)
                .wrap(Wrap { trim: false }) 
                .scroll((self.scroll, 0)), // Apply the calculated vertical offset
            history_area
        );

        frame.render_widget(&self.input_textarea, chunks[1]);
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc => self.exit(),
            KeyCode::PageUp => {
                self.auto_scroll = false;
                self.scroll = self.scroll.saturating_sub(5);
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(5);
            }
            KeyCode::Enter => {
                if self.is_generating { return; }

                let prompt = self.input_textarea.lines().join("\n");
                if prompt.trim().is_empty() { return; }

                self.is_generating = true;
                self.was_reasoning = false;
                self.auto_scroll = true;
                
                // Clear the input box
                let mut new_textarea = TextArea::default();
                new_textarea.set_block(
                    Block::bordered().title(" Generating... ").border_set(border::THICK)
                );
                new_textarea.set_cursor_line_style(Style::default());
                self.input_textarea = new_textarea;

                // Add User prompt to the display
                self.chat_display.push(Line::from(vec![
                    Span::styled("User: ", Style::default().fg(Color::Blue).bold()),
                    Span::raw(prompt.clone()),
                ]));
                
                // Add Assistant header
                self.chat_display.push(Line::from(vec![
                    Span::styled("Assistant: ", Style::default().fg(Color::Green).bold()),
                ]));

                // Setup background task variables
                let tx = self.network_tx.clone();
                let session_arc = Arc::clone(&self.session);

                tokio::spawn(async move {
                    // 1. Lock the async mutex to get a mutable reference to the session
                    let mut session = session_arc.lock().await;
                    
                    // 2. Call the chat method directly on the session
                    let _ = session.chat(&prompt, tx.clone()).await;
                    
                    // 3. Signal completion to the UI
                    let _ = tx.send(StreamEvent::Done);
                });
            }
            _ => {
                if !self.is_generating {
                    self.input_textarea.input(key_event);
                }
            }
        }
    }

    fn handle_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Reasoning(text) => {
                self.was_reasoning = true;
                self.append_text_with_newlines(&text, Style::default().fg(Color::DarkGray));
            }
            StreamEvent::Content(text) => {
                if self.was_reasoning {
                    self.chat_display.push(Line::default());
                    self.was_reasoning = false;
                }
                self.append_text_with_newlines(&text, Style::default());
            }
            StreamEvent::ToolExecution { name, args } => {
                self.chat_display.push(Line::from(vec![
                    Span::styled(format!("\n[System: Executing {} with {}]\n", name, args), Style::default().fg(Color::Cyan)),
                ]));
                self.chat_display.push(Line::default());
            }
            StreamEvent::Error(err) => {
                self.chat_display.push(Line::from(vec![
                    Span::styled(format!("\nError: {}\n", err), Style::default().fg(Color::Red).bold()),
                ]));
                self.is_generating = false;
                self.reset_textarea();
            }
            StreamEvent::Done => {
                self.chat_display.push(Line::default());
                self.is_generating = false;
                self.reset_textarea();
            }
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }
    fn reset_textarea(&mut self) {
        let mut textarea = TextArea::default();
        textarea.set_block(
            Block::bordered().title(" Prompt ").border_set(border::THICK)
        );
        textarea.set_cursor_line_style(Style::default());
        self.input_textarea = textarea;
    }
    fn append_text_with_newlines(&mut self, text: &str, style: Style) {
        let parts: Vec<&str> = text.split('\n').collect();

        for (i, part) in parts.iter().enumerate() {
            // If this is not the first chunk, it means we passed a '\n' in the string
            if i > 0 {
                self.chat_display.push(Line::default());
            }

            if !part.is_empty() {
                // Ensure there is at least one line to append to
                if self.chat_display.is_empty() {
                    self.chat_display.push(Line::default());
                }
                let last_idx = self.chat_display.len() - 1;
                self.chat_display[last_idx].spans.push(Span::styled(part.to_string(), style));
            }
        }
    }
}

impl<'a> Widget for &App<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

        // 1. Render History Area
        let history_block = Block::bordered()
            .title(Line::from(" Meta-Harness ").centered())
            .border_set(border::THICK);
            
        // Render the accumulated chat display with text wrapping enabled
        Paragraph::new(self.chat_display.clone())
            .block(history_block)
            .wrap(Wrap { trim: false }) 
            .render(chunks[0], buf);

        // 2. Render Input Area
        self.input_textarea.render(chunks[1], buf);
    }
}

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

