use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

use crate::{config::GpioConfig, ui::{theme, ModalAction}};
use super::Modal;

// ---------------------------------------------------------------------------
// Single-selection modal
// ---------------------------------------------------------------------------

/// Scrollable list where the user picks exactly one item.
pub struct SingleSelectModal {
    pub title: String,
    pub items: Vec<String>,
    pub state: ListState,
    pub on_select: Box<dyn FnOnce(Option<usize>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>)>,
}

impl SingleSelectModal {
    pub fn new(
        title: impl Into<String>,
        items: Vec<String>,
        on_select: impl FnOnce(Option<usize>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>) + 'static,
    ) -> Self {
        let mut state = ListState::default();
        if !items.is_empty() {
            state.select(Some(0));
        }
        Self {
            title: title.into(),
            items,
            state,
            on_select: Box::new(on_select),
        }
    }

    pub fn handle_key(
        mut self,
        key: KeyEvent,
        cfg: &mut GpioConfig,
    ) -> (Option<Modal>, Option<ModalAction>, bool) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.state.selected().unwrap_or(0);
                if i > 0 {
                    self.state.select(Some(i - 1));
                }
                (Some(Modal::SingleSelect(self)), None, false)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.state.selected().unwrap_or(0);
                if i + 1 < self.items.len() {
                    self.state.select(Some(i + 1));
                }
                (Some(Modal::SingleSelect(self)), None, false)
            }
            KeyCode::Enter => {
                let idx = self.state.selected();
                let (modal, action) = (self.on_select)(idx, cfg);
                (modal, action, false)
            }
            KeyCode::Esc => {
                let (modal, action) = (self.on_select)(None, cfg);
                (modal, action, false)
            }
            _ => (Some(Modal::SingleSelect(self)), None, false),
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let height = (self.items.len() as u16 + 4).min(area.height.saturating_sub(4));
        let popup = popup_rect(60, height, area);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(theme::border_normal());

        let items: Vec<ListItem> = self
            .items
            .iter()
            .map(|s| ListItem::new(Line::from(s.as_str())))
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(theme::list_selected())
            .highlight_symbol("▶ ");

        f.render_stateful_widget(list, popup, &mut self.state);
    }
}

// ---------------------------------------------------------------------------
// Multi-selection modal
// ---------------------------------------------------------------------------

/// Scrollable checklist where the user can toggle multiple items.
pub struct MultiSelectModal {
    pub title: String,
    pub items: Vec<String>,
    pub checked: Vec<bool>,
    pub cursor: usize,
    pub on_confirm: Box<dyn FnOnce(Vec<usize>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>)>,
}

impl MultiSelectModal {
    pub fn new(
        title: impl Into<String>,
        items: Vec<String>,
        pre_checked: Vec<usize>,
        on_confirm: impl FnOnce(Vec<usize>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>) + 'static,
    ) -> Self {
        let n = items.len();
        let mut checked = vec![false; n];
        for i in pre_checked {
            if i < n {
                checked[i] = true;
            }
        }
        Self {
            title: title.into(),
            items,
            checked,
            cursor: 0,
            on_confirm: Box::new(on_confirm),
        }
    }

    pub fn handle_key(
        mut self,
        key: KeyEvent,
        cfg: &mut GpioConfig,
    ) -> (Option<Modal>, Option<ModalAction>, bool) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                (Some(Modal::MultiSelect(self)), None, false)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.cursor + 1 < self.items.len() {
                    self.cursor += 1;
                }
                (Some(Modal::MultiSelect(self)), None, false)
            }
            KeyCode::Char(' ') => {
                if self.cursor < self.checked.len() {
                    self.checked[self.cursor] = !self.checked[self.cursor];
                }
                (Some(Modal::MultiSelect(self)), None, false)
            }
            KeyCode::Enter => {
                let selected: Vec<usize> = self
                    .checked
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &v)| if v { Some(i) } else { None })
                    .collect();
                let (modal, action) = (self.on_confirm)(selected, cfg);
                (modal, action, false)
            }
            KeyCode::Esc => {
                let (modal, action) = (self.on_confirm)(vec![], cfg);
                (modal, action, false)
            }
            _ => (Some(Modal::MultiSelect(self)), None, false),
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let height = (self.items.len() as u16 + 4).min(area.height.saturating_sub(4));
        let popup = popup_rect(60, height, area);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(theme::border_normal());

        let items: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let check = if self.checked[i] { "[x] " } else { "[ ] " };
                let style = if i == self.cursor {
                    theme::selected_btn()
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(format!("{check}{s}"))).style(style)
            })
            .collect();

        let list = List::new(items).block(block);
        f.render_widget(list, popup);
    }
}

// ---------------------------------------------------------------------------
// Layout helper
// ---------------------------------------------------------------------------

fn popup_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let w = area.width * percent_x / 100;
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect { x: area.x + x, y: area.y + y, width: w, height }
}
