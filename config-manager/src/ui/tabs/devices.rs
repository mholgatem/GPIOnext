/// Devices tab — lists configured virtual devices with mapping counts.
/// Keys: n = add new device via wizard, d = delete, Enter = jump to Mappings.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::{
    config::{self, GpioConfig},
    constants::DEVICE_LIST,
    ui::{
        modals::{confirm::ConfirmModal, selection::SingleSelectModal, Modal},
        ModalAction,
    },
};

pub struct DevicesTab {
    /// Rows: (device_name, mapping_count)
    pub rows: Vec<(String, usize)>,
    pub state: TableState,
}

impl DevicesTab {
    pub fn new(cfg: &GpioConfig) -> Self {
        let rows = build_rows(cfg);
        let mut state = TableState::default();
        if !rows.is_empty() {
            state.select(Some(0));
        }
        Self { rows, state }
    }

    pub fn handle_key(&mut self, key: KeyEvent, cfg: &mut GpioConfig) -> Option<Modal> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.state.selected().unwrap_or(0);
                if i > 0 {
                    self.state.select(Some(i - 1));
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.state.selected().unwrap_or(0);
                if i + 1 < self.rows.len() {
                    self.state.select(Some(i + 1));
                }
                None
            }

            // Add new device: pick any slot and navigate to Mappings to add pins.
            KeyCode::Char('n') | KeyCode::Char('N') => {
                // Show all devices, including those already configured — user may
                // want to add more mappings to an existing device.
                let items: Vec<String> = DEVICE_LIST.iter().map(|&d| d.to_owned()).collect();
                Some(Modal::SingleSelect(SingleSelectModal::new(
                    "Select Device",
                    items,
                    |idx, _cfg| {
                        if let Some(i) = idx {
                            let device = crate::constants::DEVICE_LIST[i].to_owned();
                            // Switch to Mappings tab with this device loaded.
                            (None, Some(ModalAction::RefreshMappingsTab(device)))
                        } else {
                            (None, None)
                        }
                    },
                )))
            }

            // Delete selected device
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(i) = self.state.selected() {
                    if let Some((device, _)) = self.rows.get(i) {
                        let device = device.clone();
                        return Some(Modal::Confirm(ConfirmModal::new(
                            "Delete Device",
                            format!("Delete all mappings for '{device}'?"),
                            move |yes, cfg| {
                                if yes {
                                    config::delete_device(cfg, &device);
                                    (
                                        None,
                                        Some(ModalAction::RefreshDevicesTab),
                                    )
                                } else {
                                    (None, None)
                                }
                            },
                        )));
                    }
                }
                None
            }

            _ => None,
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, cfg: &GpioConfig) {
        self.rows = build_rows(cfg);

        // Preserve cursor after config changes
        if self.rows.is_empty() {
            self.state.select(None);
        } else if self.state.selected().is_none() {
            self.state.select(Some(0));
        }

        let block = Block::default()
            .title(" Devices  [n] Go to device  [d] Delete all  [Enter] Edit mappings ")
            .borders(Borders::ALL);

        if self.rows.is_empty() {
            let hint = ratatui::widgets::Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No devices configured yet.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "  Press [n] to choose a device and start adding mappings.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "  Or go to Presets & Config to load a HAT preset.",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(block);
            f.render_widget(hint, area);
            return;
        }

        let header = Row::new(vec![
            Cell::from("Device").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Mappings").style(Style::default().add_modifier(Modifier::BOLD)),
        ])
        .style(Style::default().fg(Color::Yellow));

        let rows: Vec<Row> = self
            .rows
            .iter()
            .map(|(dev, count)| {
                Row::new(vec![
                    Cell::from(dev.as_str()),
                    Cell::from(count.to_string()),
                ])
            })
            .collect();

        let table = Table::new(rows, [Constraint::Percentage(70), Constraint::Percentage(30)])
            .header(header)
            .block(block)
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");

        f.render_stateful_widget(table, area, &mut self.state);
    }
}

fn build_rows(cfg: &GpioConfig) -> Vec<(String, usize)> {
    let mut result: Vec<(String, usize)> = Vec::new();
    for row in &cfg.devices {
        if let Some((_, count)) = result.iter_mut().find(|(d, _)| d == &row.device) {
            *count += 1;
        } else {
            result.push((row.device.clone(), 1));
        }
    }
    result
}
