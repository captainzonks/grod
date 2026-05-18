use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::io;

use crate::cast::Caster;
use crate::queue::Queue;

const HELP: &str = " space: play/pause  s: skip  d: delete  →/l: +10s  ←/h: -10s  \
                    +/-: volume  m: mute  j/k: navigate  q: quit";

pub struct TuiApp {
    queue: Queue,
    caster: Caster,
    list_state: ListState,
    status_msg: String,
}

impl TuiApp {
    pub fn new(caster: Caster) -> Result<Self> {
        let queue = Queue::open()?;
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Ok(Self {
            queue,
            caster,
            list_state,
            status_msg: HELP.to_string(),
        })
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.event_loop(&mut terminal);

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            let entries = self.queue.load()?;
            let now_playing = self.queue.now_playing()?;
            let raw_status = self.caster.status_raw().unwrap_or_default();
            let is_playing = raw_status.contains("PLAYING");
            let is_paused = raw_status.contains("PAUSED");
            let occupied = is_playing || is_paused;

            terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Min(5),
                        Constraint::Length(3),
                    ])
                    .split(f.area());

                // Now playing panel
                let (icon, suffix) = if is_playing {
                    ("▶", "")
                } else if is_paused {
                    ("⏸", " [paused]")
                } else {
                    ("■", "")
                };

                let np_text = match &now_playing {
                    Some(e) if occupied => format!(" {icon}  {}{suffix}", e.title),
                    Some(e) => format!(" ■  {} (stopped)", e.title),
                    None if occupied => format!(" {icon}  (cast outside grod){suffix}"),
                    None => " ■  Idle".to_string(),
                };

                let np_color = if is_playing {
                    Color::Green
                } else if is_paused {
                    Color::Yellow
                } else {
                    Color::DarkGray
                };

                let np = Paragraph::new(np_text)
                    .block(Block::default().borders(Borders::ALL).title("Now Playing"))
                    .style(Style::default().fg(np_color));
                f.render_widget(np, chunks[0]);

                // Queue list
                let items: Vec<ListItem> = entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("{:>3}. ", i + 1),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::raw(&e.title),
                        ]))
                    })
                    .collect();

                let queue_title = format!("Queue ({} videos)", entries.len());
                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title(queue_title))
                    .highlight_style(
                        Style::default()
                            .bg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol("▶ ");

                f.render_stateful_widget(list, chunks[1], &mut self.list_state);

                // Status / help bar
                let status = Paragraph::new(self.status_msg.as_str())
                    .block(Block::default().borders(Borders::ALL))
                    .style(Style::default().fg(Color::Yellow));
                f.render_widget(status, chunks[2]);
            })?;

            if event::poll(std::time::Duration::from_millis(500))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    let entries = self.queue.load()?;
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,

                        // Navigation
                        KeyCode::Char('j') | KeyCode::Down => {
                            let next = self
                                .list_state
                                .selected()
                                .map(|i| (i + 1).min(entries.len().saturating_sub(1)))
                                .unwrap_or(0);
                            self.list_state.select(Some(next));
                            self.status_msg = HELP.to_string();
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            let prev = self
                                .list_state
                                .selected()
                                .map(|i| i.saturating_sub(1))
                                .unwrap_or(0);
                            self.list_state.select(Some(prev));
                            self.status_msg = HELP.to_string();
                        }

                        // Queue management
                        KeyCode::Char('d') | KeyCode::Delete => {
                            if let Some(idx) = self.list_state.selected() {
                                if idx < entries.len() {
                                    match self.queue.remove(idx + 1) {
                                        Ok(e) => {
                                            let new_len = entries.len() - 1;
                                            if new_len == 0 {
                                                self.list_state.select(None);
                                            } else {
                                                self.list_state.select(Some(idx.min(new_len - 1)));
                                            }
                                            self.status_msg = format!("Removed: {}", e.title);
                                        }
                                        Err(e) => self.status_msg = format!("Error: {e}"),
                                    }
                                }
                            }
                        }
                        KeyCode::Char('s') => {
                            match self.caster.stop() {
                                Ok(_) => self.status_msg = "Skipped. Daemon will advance queue.".to_string(),
                                Err(e) => self.status_msg = format!("Error: {e}"),
                            }
                        }

                        // Playback controls
                        KeyCode::Char(' ') => {
                            match self.caster.toggle_pause() {
                                Ok(_) => self.status_msg = HELP.to_string(),
                                Err(e) => self.status_msg = format!("Error: {e}"),
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            match self.caster.seek_forward(10) {
                                Ok(_) => self.status_msg = "Seeked +10s".to_string(),
                                Err(e) => self.status_msg = format!("Error: {e}"),
                            }
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            match self.caster.seek_back(10) {
                                Ok(_) => self.status_msg = "Seeked -10s".to_string(),
                                Err(e) => self.status_msg = format!("Error: {e}"),
                            }
                        }
                        KeyCode::Char('+') => {
                            match self.caster.volume_up() {
                                Ok(_) => self.status_msg = "Volume up".to_string(),
                                Err(e) => self.status_msg = format!("Error: {e}"),
                            }
                        }
                        KeyCode::Char('-') => {
                            match self.caster.volume_down() {
                                Ok(_) => self.status_msg = "Volume down".to_string(),
                                Err(e) => self.status_msg = format!("Error: {e}"),
                            }
                        }
                        KeyCode::Char('m') => {
                            // Toggle mute based on current status
                            let raw = self.caster.status_raw().unwrap_or_default();
                            let result = if raw.contains("muted=true") {
                                self.caster.unmute()
                            } else {
                                self.caster.mute()
                            };
                            match result {
                                Ok(_) => self.status_msg = "Toggled mute".to_string(),
                                Err(e) => self.status_msg = format!("Error: {e}"),
                            }
                        }

                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }
}
