/// Presets & Config tab — HAT preset loader, JSON export/import, daemon control.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table},
    Frame,
};

use crate::{
    config::{self, GpioConfig},
    init_sys::DaemonCmd,
    presets,
    ui::{
        Modal, ModalAction,
        modals::{
            confirm::ConfirmModal,
            selection::SingleSelectModal,
            text_input::TextInputModal,
        },
    },
};

#[derive(Clone, Copy, PartialEq)]
enum Focus { Presets, DaemonControl }

pub struct PresetsConfigTab {
    focus: Focus,
    preset_state: ListState,
    daemon_cursor: usize, // 0=Start 1=Stop 2=Reload
}

impl PresetsConfigTab {
    pub fn new() -> Self {
        let mut preset_state = ListState::default();
        preset_state.select(Some(0));
        Self {
            focus: Focus::Presets,
            preset_state,
            daemon_cursor: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, cfg: &mut GpioConfig) -> Option<Modal> {
        match key.code {
            KeyCode::Tab => {
                self.focus = if self.focus == Focus::Presets {
                    Focus::DaemonControl
                } else {
                    Focus::Presets
                };
                None
            }
            _ => match self.focus {
                Focus::Presets => self.handle_presets_key(key, cfg),
                Focus::DaemonControl => self.handle_daemon_key(key, cfg),
            },
        }
    }

    fn handle_presets_key(&mut self, key: KeyEvent, cfg: &mut GpioConfig) -> Option<Modal> {
        let n = presets::PRESETS.len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.preset_state.selected().unwrap_or(0);
                if i > 0 { self.preset_state.select(Some(i - 1)); }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.preset_state.selected().unwrap_or(0);
                if i + 1 < n { self.preset_state.select(Some(i + 1)); }
                None
            }
            KeyCode::Enter => {
                let idx = self.preset_state.selected()?;
                let preset = presets::PRESETS[idx].key;
                let display = presets::PRESETS[idx].display_name;
                Some(Modal::Confirm(ConfirmModal::new(
                    "Load Preset",
                    format!("Load '{display}'? This will overwrite existing Joypad 1 mappings."),
                    move |yes, cfg| {
                        if yes {
                            config::delete_device(cfg, "Joypad 1");
                            for row in presets::get_preset_rows(preset) {
                                config::upsert_mapping(cfg, row);
                            }
                            return (None, Some(ModalAction::Save));
                        }
                        (None, None)
                    },
                )))
            }
            // Export JSON
            KeyCode::Char('e') | KeyCode::Char('E') => {
                Some(Modal::TextInput(TextInputModal::new(
                    "Export Config",
                    "Output file path:",
                    "./gpionext_backup.json",
                    |path, cfg| {
                        if let Some(p) = path {
                            match serde_json::to_string_pretty(cfg) {
                                Ok(json) => {
                                    match std::fs::write(&p, json) {
                                        Ok(_) => return (None, Some(ModalAction::StatusMsg(format!("Exported to {p}")))),
                                        Err(e) => return (None, Some(ModalAction::StatusMsg(format!("Export failed: {e}")))),
                                    }
                                }
                                Err(e) => return (None, Some(ModalAction::StatusMsg(format!("Serialise error: {e}")))),
                            }
                        }
                        (None, None)
                    },
                )))
            }
            // Import JSON
            KeyCode::Char('i') | KeyCode::Char('I') => {
                Some(Modal::TextInput(TextInputModal::new(
                    "Import Config",
                    "JSON file path:",
                    "./gpionext_backup.json",
                    |path, cfg| {
                        if let Some(p) = path {
                            match std::fs::read_to_string(&p) {
                                Ok(data) => {
                                    match serde_json::from_str::<GpioConfig>(&data) {
                                        Ok(imported) => {
                                            *cfg = imported;
                                            return (None, Some(ModalAction::Save));
                                        }
                                        Err(e) => return (None, Some(ModalAction::StatusMsg(format!("Parse error: {e}")))),
                                    }
                                }
                                Err(e) => return (None, Some(ModalAction::StatusMsg(format!("Read error: {e}")))),
                            }
                        }
                        (None, None)
                    },
                )))
            }
            _ => None,
        }
    }

    fn handle_daemon_key(&mut self, key: KeyEvent, _cfg: &mut GpioConfig) -> Option<Modal> {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                if self.daemon_cursor > 0 { self.daemon_cursor -= 1; }
                None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.daemon_cursor < 2 { self.daemon_cursor += 1; }
                None
            }
            KeyCode::Enter => {
                let cmd = match self.daemon_cursor {
                    0 => DaemonCmd::Start,
                    1 => DaemonCmd::Stop,
                    _ => DaemonCmd::Reload,
                };
                Some(Modal::Confirm(ConfirmModal::new(
                    "Daemon Control",
                    format!("{cmd:?} gpionext daemon?"),
                    move |yes, _cfg| {
                        if yes {
                            return (None, Some(ModalAction::DaemonAction(cmd)));
                        }
                        (None, None)
                    },
                )))
            }
            _ => None,
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, _cfg: &GpioConfig) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(60), // presets list
                Constraint::Length(5),      // daemon control
                Constraint::Min(0),         // export/import hints
            ])
            .split(area);

        self.render_presets(f, chunks[0]);
        self.render_daemon_control(f, chunks[1]);
        self.render_hints(f, chunks[2]);
    }

    fn render_presets(&mut self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = presets::PRESETS
            .iter()
            .map(|p| ListItem::new(Line::from(p.display_name)))
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" HAT Presets  [↑↓] select  [Enter] load  [e] Export  [i] Import ")
                    .borders(Borders::ALL)
                    .border_style(if self.focus == Focus::Presets {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default()
                    }),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        f.render_stateful_widget(list, area, &mut self.preset_state);
    }

    fn render_daemon_control(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Daemon Control  [Tab] focus  [←→] select  [Enter] run ")
            .borders(Borders::ALL)
            .border_style(if self.focus == Focus::DaemonControl {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        let btn_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(34),
            ])
            .split(inner);

        let labels = ["  Start  ", "  Stop  ", "  Reload  "];
        for (i, (label, chunk)) in labels.iter().zip(btn_chunks.iter()).enumerate() {
            let style = if self.focus == Focus::DaemonControl && self.daemon_cursor == i {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            f.render_widget(
                Paragraph::new(Line::from(*label))
                    .style(style)
                    .block(Block::default().borders(Borders::ALL)),
                *chunk,
            );
        }
    }

    fn render_hints(&self, f: &mut Frame, area: Rect) {
        f.render_widget(
            Paragraph::new(vec![
                Line::from("  [e] Export config to JSON file"),
                Line::from("  [i] Import config from JSON file"),
            ])
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
}
