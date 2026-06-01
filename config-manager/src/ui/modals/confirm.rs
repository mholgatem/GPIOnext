use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::{config::GpioConfig, ui::{theme, ModalAction}};
use super::Modal;

/// A simple yes/no confirmation dialog.
pub struct ConfirmModal {
    pub title: String,
    pub message: String,
    /// Called with `true` (yes) or `false` (no) to produce the next state.
    pub on_confirm: Box<dyn FnOnce(bool, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>)>,
}

impl ConfirmModal {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        on_confirm: impl FnOnce(bool, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>) + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            on_confirm: Box::new(on_confirm),
        }
    }

    pub fn handle_key(
        self,
        key: KeyEvent,
        cfg: &mut GpioConfig,
    ) -> (Option<Modal>, Option<ModalAction>, bool) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let (modal, action) = (self.on_confirm)(true, cfg);
                (modal, action, false)
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                let (modal, action) = (self.on_confirm)(false, cfg);
                (modal, action, false)
            }
            _ => (Some(Modal::Confirm(self)), None, false),
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let popup = centered_rect(50, 7, area);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(theme::border_focused());

        let inner = block.inner(popup);
        f.render_widget(block, popup);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(inner);

        let msg = Paragraph::new(Line::from(self.message.as_str()))
            .alignment(Alignment::Center);
        f.render_widget(msg, chunks[0]);

        let hint = Paragraph::new(Line::from("  [Y] Yes   [N] No"))
            .style(theme::hint_text().add_modifier(Modifier::ITALIC));
        f.render_widget(hint, chunks[1]);
    }
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_width = area.width * percent_x / 100;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect {
        x: area.x + x,
        y: area.y + y,
        width: popup_width,
        height,
    }
}
