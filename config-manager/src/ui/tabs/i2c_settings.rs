/// I2C & Settings tab — daemon settings + I2C chip table + live pin view.
///
/// Three sub-panels, cycled with F1/F2/F3:
///   [F1] Daemon Settings  — ↑↓ move, Enter/e edit, Space toggle
///   [F2] I2C Chips        — ↑↓ move, a add, d delete
///   [F3] Live Pin Monitor — ↑↓ scroll through all pins

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};
use std::sync::{Arc, Mutex};

use crate::{
    config::{Ads1115Entry, GpioConfig, Mcp23017Entry, Pcf8574Entry},
    ipc_client::PinState,
    ui::{
        live_pin_view,
        modals::{
            add_i2c::{AddI2cModal, ChipType, I2cEntry},
            confirm::ConfirmModal,
            Modal,
        },
        theme, ModalAction,
    },
};
use super::TabHint;

#[derive(Clone, Copy, PartialEq)]
enum Focus { Settings, I2cTable, LivePins }

pub struct I2cSettingsTab {
    focus: Focus,
    i2c_state: TableState,
    settings_field: usize,
    edit_buf: String,
    editing: bool,
    /// Scroll state for the live pin monitor (F3 panel).
    live_pin_scroll: TableState,
}

impl I2cSettingsTab {
    pub fn new(_cfg: &GpioConfig) -> Self {
        let mut i2c_state = TableState::default();
        i2c_state.select(Some(0));
        Self {
            focus: Focus::Settings,
            i2c_state,
            settings_field: 0,
            edit_buf: String::new(),
            editing: false,
            live_pin_scroll: TableState::default(),
        }
    }

    pub fn tick(&mut self) {}

    pub fn handle_key(&mut self, key: KeyEvent, cfg: &mut GpioConfig) -> Option<Modal> {
        match key.code {
            KeyCode::F(1) => { self.focus = Focus::Settings;  None }
            KeyCode::F(2) => { self.focus = Focus::I2cTable;  None }
            KeyCode::F(3) => { self.focus = Focus::LivePins;  None }
            _ => match self.focus {
                Focus::Settings  => self.handle_settings_key(key, cfg),
                Focus::I2cTable  => self.handle_i2c_key(key, cfg),
                Focus::LivePins  => self.handle_live_pin_key(key, cfg),
            },
        }
    }

    fn handle_settings_key(&mut self, key: KeyEvent, cfg: &mut GpioConfig) -> Option<Modal> {
        const N_FIELDS: usize = 7;
        if self.editing {
            match key.code {
                KeyCode::Enter | KeyCode::Esc => {
                    self.commit_edit(cfg);
                    self.editing = false;
                }
                // Backspace variants
                KeyCode::Backspace | KeyCode::Char('\x7f') | KeyCode::Char('\x08') => {
                    self.edit_buf.pop();
                }
                // Ctrl+H = backspace in many terminals
                KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.edit_buf.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.edit_buf.push(c);
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Up => {
                    if self.settings_field > 0 { self.settings_field -= 1; }
                }
                KeyCode::Down => {
                    if self.settings_field + 1 < N_FIELDS { self.settings_field += 1; }
                }
                KeyCode::Enter | KeyCode::Char('e') => {
                    self.edit_buf = self.current_field_value(cfg);
                    self.editing = true;
                }
                KeyCode::Char(' ') => { self.toggle_bool_field(cfg); }
                _ => {}
            }
        }
        None
    }

    fn handle_live_pin_key(&mut self, key: KeyEvent, cfg: &GpioConfig) -> Option<Modal> {
        let total = live_pin_view::total_rows(cfg);
        match key.code {
            KeyCode::Up => {
                let i = self.live_pin_scroll.selected().unwrap_or(0);
                self.live_pin_scroll.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Down => {
                let i = self.live_pin_scroll.selected().unwrap_or(0);
                if i + 1 < total {
                    self.live_pin_scroll.select(Some(i + 1));
                }
            }
            _ => {}
        }
        None
    }

    fn current_field_value(&self, cfg: &GpioConfig) -> String {
        let d = &cfg.daemon;
        match self.settings_field {
            0 => d.combo_delay.to_string(),
            1 => d.key_hold_delay.to_string(),
            2 => d.debounce.to_string(),
            3 => d.pins.clone(),
            4 => d.pulldown.to_string(),
            5 => d.dev.to_string(),
            6 => d.debug.to_string(),
            _ => String::new(),
        }
    }

    fn commit_edit(&self, cfg: &mut GpioConfig) {
        let d = &mut cfg.daemon;
        match self.settings_field {
            0 => { if let Ok(v) = self.edit_buf.parse() { d.combo_delay    = v; } }
            1 => { if let Ok(v) = self.edit_buf.parse() { d.key_hold_delay = v; } }
            2 => { if let Ok(v) = self.edit_buf.parse() { d.debounce       = v; } }
            3 => { d.pins = self.edit_buf.clone(); }
            4 => { d.pulldown = self.edit_buf == "true"; }
            5 => { d.dev      = self.edit_buf == "true"; }
            6 => { d.debug    = self.edit_buf == "true"; }
            _ => {}
        }
    }

    fn toggle_bool_field(&mut self, cfg: &mut GpioConfig) {
        let d = &mut cfg.daemon;
        match self.settings_field {
            4 => d.pulldown = !d.pulldown,
            5 => d.dev      = !d.dev,
            6 => d.debug    = !d.debug,
            _ => {}
        }
    }

    fn handle_i2c_key(&mut self, key: KeyEvent, cfg: &mut GpioConfig) -> Option<Modal> {
        let chip_count = i2c_rows_count(cfg);
        match key.code {
            KeyCode::Up => {
                let i = self.i2c_state.selected().unwrap_or(0);
                if i > 0 { self.i2c_state.select(Some(i - 1)); }
                None
            }
            KeyCode::Down => {
                let i = self.i2c_state.selected().unwrap_or(0);
                if i + 1 < chip_count { self.i2c_state.select(Some(i + 1)); }
                None
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                Some(Modal::AddI2c(AddI2cModal::new(|entry, cfg| {
                    if let Some(e) = entry { apply_i2c_entry(e, cfg); }
                    (None, Some(ModalAction::Save))
                })))
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(i) = self.i2c_state.selected() {
                    let label = i2c_row_label(cfg, i);
                    return Some(Modal::Confirm(ConfirmModal::new(
                        "Remove I2C Device",
                        format!("Remove '{label}'?"),
                        move |yes, cfg| {
                            if yes { remove_i2c_by_index(cfg, i); }
                            (None, Some(ModalAction::Save))
                        },
                    )));
                }
                None
            }
            _ => None,
        }
    }

    pub fn render(
        &mut self,
        f: &mut Frame,
        area: Rect,
        cfg: &GpioConfig,
        pin_state: &Arc<Mutex<PinState>>,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(12),
                Constraint::Min(0),
            ])
            .split(area);

        let top_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(chunks[0]);

        self.render_settings(f, top_chunks[0], cfg);
        self.render_i2c_table(f, top_chunks[1], cfg);
        live_pin_view::render(
            f,
            chunks[1],
            cfg,
            pin_state,
            &mut self.live_pin_scroll,
            self.focus == Focus::LivePins,
        );
    }

    fn render_settings(&self, f: &mut Frame, area: Rect, cfg: &GpioConfig) {
        let d = &cfg.daemon;
        let fields: &[(&str, String)] = &[
            ("Combo delay (ms)",    d.combo_delay.to_string()),
            ("Key hold delay (ms)", d.key_hold_delay.to_string()),
            ("Debounce (ms)",       d.debounce.to_string()),
            ("Active pins",         d.pins.clone()),
            ("Pull-down",           d.pulldown.to_string()),
            ("Dev mode",            d.dev.to_string()),
            ("Debug logging",       d.debug.to_string()),
        ];

        let rows: Vec<Row> = fields
            .iter()
            .enumerate()
            .map(|(i, (label, value))| {
                let val_display = if self.editing && self.settings_field == i {
                    format!("{}_", self.edit_buf)
                } else {
                    value.clone()
                };
                let row_style = if self.focus == Focus::Settings && self.settings_field == i {
                    theme::selected_row()
                } else {
                    Style::default()
                };
                Row::new(vec![
                    Cell::from(*label).style(row_style),
                    Cell::from(val_display).style(row_style),
                ])
            })
            .collect();

        let focused = self.focus == Focus::Settings;
        let table = Table::new(
            rows,
            [Constraint::Percentage(55), Constraint::Percentage(45)],
        )
        .block(
            Block::default()
                .title(" Settings [F1] ")
                .borders(Borders::ALL)
                .border_style(if focused { theme::border_focused() } else { theme::border_normal() }),
        );

        f.render_widget(table, area);
    }

    fn render_i2c_table(&mut self, f: &mut Frame, area: Rect, cfg: &GpioConfig) {
        let focused = self.focus == Focus::I2cTable;
        let block = Block::default()
            .title(" I2C Chips [F2] ")
            .borders(Borders::ALL)
            .border_style(if focused { theme::border_focused() } else { theme::border_normal() });

        if i2c_rows_count(cfg) == 0 {
            let inner = block.inner(area);
            f.render_widget(block, area);
            if focused {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "No I2C chips configured. Press [a] to configure.",
                        theme::hint_text(),
                    )))
                    .alignment(Alignment::Center),
                    inner,
                );
            }
            return;
        }

        let rows = build_i2c_rows(cfg);
        let header = Row::new(vec![
            Cell::from("Chip").style(theme::header().add_modifier(Modifier::BOLD)),
            Cell::from("Bus").style(theme::header().add_modifier(Modifier::BOLD)),
            Cell::from("Address").style(theme::header().add_modifier(Modifier::BOLD)),
            Cell::from("Int Pin").style(theme::header().add_modifier(Modifier::BOLD)),
        ]);

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(35),
                Constraint::Percentage(15),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ],
        )
        .header(header)
        .block(block)
        .highlight_style(theme::selected_row());

        f.render_stateful_widget(table, area, &mut self.i2c_state);
    }
}

impl TabHint for I2cSettingsTab {
    fn hint(&self) -> &str {
        match self.focus {
            Focus::Settings => {
                "Settings [F1] | ↑↓: move  Enter/e: edit  Space: toggle  F2: I2C Chips  F3: Live Pins"
            }
            Focus::I2cTable => {
                "I2C Chips [F2] | ↑↓: move  a: add  d: delete  F1: Settings  F3: Live Pins"
            }
            Focus::LivePins => {
                "Live Pins [F3] | ↑↓: scroll  F1: Settings  F2: I2C Chips"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// I2C table helpers
// ---------------------------------------------------------------------------

fn build_i2c_rows(cfg: &GpioConfig) -> Vec<Row> {
    let mut rows = Vec::new();
    for e in &cfg.i2c.mcp23017 {
        rows.push(Row::new(vec![
            Cell::from("MCP23017"),
            Cell::from(e.bus.to_string()),
            Cell::from(e.address.as_str()),
            Cell::from(e.int_pin.as_str()),
        ]));
    }
    for e in &cfg.i2c.ads1115 {
        rows.push(Row::new(vec![
            Cell::from("ADS1115"),
            Cell::from(e.bus.to_string()),
            Cell::from(e.address.as_str()),
            Cell::from("—"),
        ]));
    }
    for e in &cfg.i2c.pcf8574 {
        rows.push(Row::new(vec![
            Cell::from("PCF8574"),
            Cell::from(e.bus.to_string()),
            Cell::from(e.address.as_str()),
            Cell::from(e.int_pin.as_str()),
        ]));
    }
    rows
}

fn i2c_rows_count(cfg: &GpioConfig) -> usize {
    cfg.i2c.mcp23017.len() + cfg.i2c.ads1115.len() + cfg.i2c.pcf8574.len()
}

fn i2c_row_label(cfg: &GpioConfig, idx: usize) -> String {
    let mcp_len = cfg.i2c.mcp23017.len();
    let ads_len = cfg.i2c.ads1115.len();
    if idx < mcp_len {
        format!("MCP23017 {}", cfg.i2c.mcp23017[idx].address)
    } else if idx < mcp_len + ads_len {
        format!("ADS1115 {}", cfg.i2c.ads1115[idx - mcp_len].address)
    } else {
        format!("PCF8574 {}", cfg.i2c.pcf8574[idx - mcp_len - ads_len].address)
    }
}

fn remove_i2c_by_index(cfg: &mut GpioConfig, idx: usize) {
    let mcp_len = cfg.i2c.mcp23017.len();
    let ads_len = cfg.i2c.ads1115.len();
    if idx < mcp_len {
        cfg.i2c.mcp23017.remove(idx);
    } else if idx < mcp_len + ads_len {
        cfg.i2c.ads1115.remove(idx - mcp_len);
    } else {
        let pcf_idx = idx - mcp_len - ads_len;
        if pcf_idx < cfg.i2c.pcf8574.len() {
            cfg.i2c.pcf8574.remove(pcf_idx);
        }
    }
}

fn apply_i2c_entry(e: I2cEntry, cfg: &mut GpioConfig) {
    match e.chip {
        ChipType::Mcp23017 => cfg.i2c.mcp23017.push(Mcp23017Entry {
            bus: e.bus,
            address: e.address,
            int_pin: e.int_pin,
        }),
        ChipType::Ads1115 => cfg.i2c.ads1115.push(Ads1115Entry {
            bus: e.bus,
            address: e.address,
        }),
        ChipType::Pcf8574 => cfg.i2c.pcf8574.push(Pcf8574Entry {
            bus: e.bus,
            address: e.address,
            int_pin: e.int_pin,
        }),
    }
}
