/// I2C & Settings tab — daemon settings + I2C chip table + live pin view.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};
use std::sync::{Arc, Mutex};

use crate::{
    config::{Ads1115Entry, GpioConfig, Mcp23017Entry, Pcf8574Entry},
    ipc_client::PinState,
    ui::{
        Modal, ModalAction,
        modals::{
            add_i2c::{AddI2cModal, ChipType, I2cEntry},
            confirm::ConfirmModal,
        },
    },
};
use super::super::live_pin_view;

#[derive(Clone, Copy, PartialEq)]
enum Focus { Settings, I2cTable, LivePins }

pub struct I2cSettingsTab {
    focus: Focus,
    i2c_state: TableState,
    /// Editable settings field index (0-6 matching DaemonSettings fields)
    settings_field: usize,
    /// Temp buffer for editing the current settings field text
    edit_buf: String,
    editing: bool,
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
        }
    }

    pub fn tick(&mut self) {
        // Live pin view refreshes passively from Arc<Mutex<PinState>> — no tick state needed here
    }

    pub fn handle_key(&mut self, key: KeyEvent, cfg: &mut GpioConfig) -> Option<Modal> {
        match key.code {
            // Switch focus between panels
            KeyCode::F(1) => { self.focus = Focus::Settings; None }
            KeyCode::F(2) => { self.focus = Focus::I2cTable; None }
            KeyCode::F(3) => { self.focus = Focus::LivePins; None }

            _ => match self.focus {
                Focus::Settings => self.handle_settings_key(key, cfg),
                Focus::I2cTable => self.handle_i2c_key(key, cfg),
                Focus::LivePins => None,
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
                KeyCode::Backspace => { self.edit_buf.pop(); }
                KeyCode::Char(c) => { self.edit_buf.push(c); }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if self.settings_field > 0 { self.settings_field -= 1; }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.settings_field + 1 < N_FIELDS { self.settings_field += 1; }
                }
                KeyCode::Enter | KeyCode::Char('e') => {
                    self.edit_buf = self.current_field_value(cfg);
                    self.editing = true;
                }
                // Toggle bool fields with space
                KeyCode::Char(' ') => {
                    self.toggle_bool_field(cfg);
                }
                _ => {}
            }
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
            0 => { if let Ok(v) = self.edit_buf.parse() { d.combo_delay = v; } }
            1 => { if let Ok(v) = self.edit_buf.parse() { d.key_hold_delay = v; } }
            2 => { if let Ok(v) = self.edit_buf.parse() { d.debounce = v; } }
            3 => { d.pins = self.edit_buf.clone(); }
            4 => { d.pulldown = self.edit_buf == "true"; }
            5 => { d.dev = self.edit_buf == "true"; }
            6 => { d.debug = self.edit_buf == "true"; }
            _ => {}
        }
    }

    fn toggle_bool_field(&mut self, cfg: &mut GpioConfig) {
        let d = &mut cfg.daemon;
        match self.settings_field {
            4 => d.pulldown = !d.pulldown,
            5 => d.dev = !d.dev,
            6 => d.debug = !d.debug,
            _ => {}
        }
    }

    fn handle_i2c_key(&mut self, key: KeyEvent, cfg: &mut GpioConfig) -> Option<Modal> {
        let chip_count = i2c_rows_count(cfg);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.i2c_state.selected().unwrap_or(0);
                if i > 0 { self.i2c_state.select(Some(i - 1)); }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.i2c_state.selected().unwrap_or(0);
                if i + 1 < chip_count { self.i2c_state.select(Some(i + 1)); }
                None
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                Some(Modal::AddI2c(AddI2cModal::new(|entry, cfg| {
                    if let Some(e) = entry {
                        apply_i2c_entry(e, cfg);
                    }
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
                Constraint::Length(12), // top: settings + i2c table side by side
                Constraint::Min(0),     // bottom: live pin view
            ])
            .split(area);

        let top_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(chunks[0]);

        self.render_settings(f, top_chunks[0], cfg);
        self.render_i2c_table(f, top_chunks[1], cfg);
        live_pin_view::render(f, chunks[1], cfg, pin_state);

        // Focus indicator
        let hint = match self.focus {
            Focus::Settings  => "F1: Settings  F2: I2C Chips  F3: Live Pins  |  ↑↓ move  Enter/Space: edit",
            Focus::I2cTable  => "F1: Settings  F2: I2C Chips  F3: Live Pins  |  n: Add  d: Delete",
            Focus::LivePins  => "F1: Settings  F2: I2C Chips  F3: Live Pins",
        };
        // Hint is rendered by App's status bar via tab context; nothing to do here
        let _ = hint;
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
                let style = if self.focus == Focus::Settings && self.settings_field == i {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                Row::new(vec![
                    Cell::from(*label).style(style),
                    Cell::from(val_display).style(style),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [Constraint::Percentage(55), Constraint::Percentage(45)],
        )
        .block(
            Block::default()
                .title(" Daemon Settings  [↑↓] move  [Enter] edit  [Space] toggle ")
                .borders(Borders::ALL)
                .border_style(if self.focus == Focus::Settings {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                }),
        );

        f.render_widget(table, area);
    }

    fn render_i2c_table(&mut self, f: &mut Frame, area: Rect, cfg: &GpioConfig) {
        let rows = build_i2c_rows(cfg);
        let header = Row::new(vec![
            Cell::from("Chip").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Bus").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Address").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Int Pin").style(Style::default().add_modifier(Modifier::BOLD)),
        ])
        .style(Style::default().fg(Color::Yellow));

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
        .block(
            Block::default()
                .title(" I2C Chips  [n] Add  [d] Delete ")
                .borders(Borders::ALL)
                .border_style(if self.focus == Focus::I2cTable {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                }),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

        f.render_stateful_widget(table, area, &mut self.i2c_state);
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
