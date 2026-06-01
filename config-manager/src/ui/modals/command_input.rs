use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::{config::GpioConfig, ui::{theme, ModalAction}};
use super::Modal;

#[derive(Clone, Copy, PartialEq)]
enum Field { Command, Timeout }

/// Two-field modal: shell command string + optional timeout (seconds).
pub struct CommandInputModal {
    pub title: String,
    pub command: String,
    pub timeout: String,
    active_field: Field,
    pub on_confirm: Box<dyn FnOnce(Option<(String, u32)>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>)>,
}

impl CommandInputModal {
    pub fn new(
        title: impl Into<String>,
        on_confirm: impl FnOnce(Option<(String, u32)>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>) + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            command: String::new(),
            timeout: "0".into(),
            active_field: Field::Command,
            on_confirm: Box::new(on_confirm),
        }
    }

    pub fn handle_key(
        mut self,
        key: KeyEvent,
        cfg: &mut GpioConfig,
    ) -> (Option<Modal>, Option<ModalAction>, bool) {
        match key.code {
            KeyCode::Esc => {
                let (modal, action) = (self.on_confirm)(None, cfg);
                (modal, action, false)
            }
            KeyCode::Tab => {
                self.active_field = if self.active_field == Field::Command {
                    Field::Timeout
                } else {
                    Field::Command
                };
                (Some(Modal::CommandInput(self)), None, false)
            }
            KeyCode::Enter => {
                let timeout: u32 = self.timeout.parse().unwrap_or(0);
                if self.command.is_empty() {
                    (Some(Modal::CommandInput(self)), None, false)
                } else {
                    let (modal, action) = (self.on_confirm)(Some((self.command, timeout)), cfg);
                    (modal, action, false)
                }
            }
            // Handle all backspace variants
            KeyCode::Backspace | KeyCode::Char('\x08') | KeyCode::Char('\x7f') => {
                match self.active_field {
                    Field::Command => { self.command.pop(); }
                    Field::Timeout => { self.timeout.pop(); }
                }
                (Some(Modal::CommandInput(self)), None, false)
            }
            KeyCode::Char(c) => {
                match self.active_field {
                    Field::Command => self.command.push(c),
                    Field::Timeout => {
                        if c.is_ascii_digit() { self.timeout.push(c); }
                    }
                }
                (Some(Modal::CommandInput(self)), None, false)
            }
            _ => (Some(Modal::CommandInput(self)), None, false),
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let popup = centered_rect(70, 9, area);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(theme::border_normal());
        let inner = block.inner(popup);
        f.render_widget(block, popup);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Length(2),
                Constraint::Min(0),
            ])
            .split(inner);

        let active_style = theme::input_text();
        let inactive_style = Style::default().fg(theme::CYAN);

        let cmd_label_style = if self.active_field == Field::Command { active_style } else { inactive_style };
        f.render_widget(Paragraph::new("Command:").style(cmd_label_style), chunks[0]);

        let cmd_field_style = if self.active_field == Field::Command { active_style } else { inactive_style };
        f.render_widget(
            Paragraph::new(format!("> {}_", self.command))
                .block(Block::default().borders(Borders::BOTTOM).border_style(theme::border_normal()))
                .style(cmd_field_style),
            chunks[1],
        );

        let to_label_style = if self.active_field == Field::Timeout { active_style } else { inactive_style };
        f.render_widget(Paragraph::new("Timeout (s, 0 = none):").style(to_label_style), chunks[2]);

        let to_field_style = if self.active_field == Field::Timeout { active_style } else { inactive_style };
        f.render_widget(
            Paragraph::new(format!("> {}_", self.timeout))
                .block(Block::default().borders(Borders::BOTTOM).border_style(theme::border_normal()))
                .style(to_field_style),
            chunks[3],
        );

        f.render_widget(
            Paragraph::new(Line::from("  Tab: switch field   Enter: confirm   Esc: cancel"))
                .style(theme::hint_text()),
            chunks[4],
        );
    }
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let w = area.width * percent_x / 100;
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect { x: area.x + x, y: area.y + y, width: w, height }
}
