use std::io::{self};
use std::sync::Arc;

use crate::events::{handle_stream_event, handle_user_event};
use crate::llm_client::LlmClient;
use crate::llm_client::provider::LlmProvider;
use crate::session::{SessionData, StreamEvent, Task};
use crate::ui::draw;
use crossterm::event::EventStream;
use futures::StreamExt;
use ratatui::{DefaultTerminal, style::Style, symbols::border, text::Line, widgets::Block};
use ratatui_textarea::TextArea;
use tokio::select;
use tokio::sync::{Mutex, mpsc};

pub struct App<'a, P: LlmProvider + Send + Sync + 'static> {
    pub session: Arc<Mutex<SessionData>>,
    pub llm_client: Arc<LlmClient<P>>,

    exit: bool,
    pub is_generating: bool,
    pub was_reasoning: bool,
    pub plan: Vec<Task>,

    pub active_reasoning_buffer: String,
    pub active_content_buffer: String,

    pub network_tx: mpsc::UnboundedSender<StreamEvent>,
    pub network_rx: mpsc::UnboundedReceiver<StreamEvent>,

    pub input_textarea: TextArea<'a>,
    pub chat_display: Vec<Line<'static>>,

    pub scroll: u16,
    pub auto_scroll: bool,
}

impl<'a, P: LlmProvider + Send + Sync + 'static> App<'a, P> {
    pub fn new(session: Arc<Mutex<SessionData>>, llm_client: LlmClient<P>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut textarea = TextArea::default();
        textarea.set_block(
            Block::bordered()
                .title(" Prompt ")
                .border_set(border::THICK),
        );
        textarea.set_cursor_line_style(Style::default());

        let initial_plan = {
            if let Ok(session_guard) = session.try_lock() {
                session_guard.plan.clone()
            } else {
                Vec::new()
            }
        };

        Self {
            session: session,
            llm_client: Arc::new(llm_client),
            exit: false,
            plan: initial_plan,

            is_generating: false,
            was_reasoning: false,
            active_reasoning_buffer: String::new(),
            active_content_buffer: String::new(),
            input_textarea: textarea,
            network_tx: tx,
            network_rx: rx,
            chat_display: Vec::new(),
            scroll: 0,
            auto_scroll: true,
        }
    }

    /// runs the application's main loop until the user quits
    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let mut event_reader = EventStream::new();
        while !self.exit {
            terminal.draw(|frame| draw(self, frame))?;

            select! {
                Some(Ok(event)) = event_reader.next() => {
                    handle_user_event(self, event);
                }

                Some(stream_event) = self.network_rx.recv() => {
                    handle_stream_event(self, stream_event);
                }
            }
        }
        Ok(())
    }

    pub fn exit(&mut self) {
        self.exit = true;
    }
}
