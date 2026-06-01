/// Devices tab — lists configured virtual devices with mapping counts.
/// Keys: n = add new device via wizard, d = delete, Enter = jump to Mappings.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame,
};

use crate::{
    config::{self, GpioConfig},
    constants::DEVICE_LIST,
    ui::{
        Modal, ModalAction,
        modals::{
            confirm::ConfirmModal,
            selection::SingleSelectModal,
        },
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

            // Add new device: pick from DEVICE_LIST items that have no mappings yet
            KeyCode::Char('n') | KeyCode::Char('N') => {
                let active: std::collections::HashSet<String> =
                    config::active_devices(cfg).into_iter().collect();
                let available: Vec<String> = DEVICE_LIST
                    .iter()
                    .filter(|&&d| !active.contains(d))
                    .map(|&d| d.to_owned())
                    .collect();

                if available.is_empty() {
                    return None; // all devices already configured
                }

                Some(Modal::SingleSelect(SingleSelectModal::new(
                    "Select Device to Add",
                    available,
                    |idx, _cfg| {
                        // The caller (app.rs apply_modal_action) will switch to
                        // Mappings tab; for now just return a refresh action.
                        if idx.is_some() {
                            (None, Some(ModalAction::RefreshDevicesTab))
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
        // Refresh row data on every render (cheap Vec scan)
        self.rows = build_rows(cfg);

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

        let table = Table::new(
            rows,
            [Constraint::Percentage(70), Constraint::Percentage(30)],
        )
        .header(header)
        .block(
            Block::default()
                .title(" Devices  [n] Add  [d] Delete  [Enter] Edit mappings ")
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
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
