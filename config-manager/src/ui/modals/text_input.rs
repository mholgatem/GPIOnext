use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::{config::GpioConfig, ui::{theme, ModalAction}};
use super::Modal;

/// Generic single-line text input modal.
pub struct TextInputModal {
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub on_confirm: Box<dyn FnOnce(Option<String>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>)>,
}

impl TextInputModal {
    pub fn new(
        title: impl Into<String>,
        prompt: impl Into<String>,
        initial: impl Into<String>,
        on_confirm: impl FnOnce(Option<String>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>) + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            prompt: prompt.into(),
            value: initial.into(),
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
            KeyCode::Enter => {
                let val = if self.value.is_empty() { None } else { Some(self.value) };
                let (modal, action) = (self.on_confirm)(val, cfg);
                (modal, action, false)
            }
            // Backspace variants: physical key, \x7f (DEL-as-backspace), \x08 literal
            KeyCode::Backspace | KeyCode::Char('\x7f') | KeyCode::Char('\x08') => {
                self.value.pop();
                (Some(Modal::TextInput(self)), None, false)
            }
            // Ctrl+H = backspace in many terminal emulators (crossterm decodes as Char('h') + CONTROL)
            KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.value.pop();
                (Some(Modal::TextInput(self)), None, false)
            }
            // Normal printable character — ignore any Ctrl-modified input
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.value.push(c);
                (Some(Modal::TextInput(self)), None, false)
            }
            _ => (Some(Modal::TextInput(self)), None, false),
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let popup = centered_rect(60, 7, area);
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
                Constraint::Min(0),
            ])
            .split(inner);

        f.render_widget(
            Paragraph::new(self.prompt.as_str()).style(Style::default().fg(theme::CYAN)),
            chunks[0],
        );
        f.render_widget(
            Paragraph::new(format!("> {}_", self.value))
                .block(Block::default().borders(Borders::BOTTOM).border_style(theme::border_normal()))
                .style(theme::input_text()),
            chunks[1],
        );
        f.render_widget(
            Paragraph::new(Line::from("Enter: confirm   Esc: cancel"))
                .style(theme::hint_text()),
            chunks[2],
        );
    }
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let w = area.width * percent_x / 100;
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect { x: area.x + x, y: area.y + y, width: w, height }
}
