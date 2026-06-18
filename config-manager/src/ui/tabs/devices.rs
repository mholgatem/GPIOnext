/// Devices tab — 2×3 grid of device "buttons" for Joypad 1-4, Keyboard, Commands.
/// Keys: ←→↑↓ navigate grid; Enter = edit mappings for selected; d = delete device.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::{HashMap, HashSet};

use crate::{
    config::{self, GpioConfig},
    constants::DEVICE_LIST,
    ui::{
        modals::{confirm::ConfirmModal, Modal},
        theme, ModalAction,
    },
};
use super::TabHint;

pub struct DevicesTab {
    /// Index into DEVICE_LIST (0-5) of the currently focused button.
    pub selected: usize,
}

impl DevicesTab {
    pub fn new(_cfg: &GpioConfig) -> Self {
        Self { selected: 0 }
    }

    pub fn handle_key(&mut self, key: KeyEvent, cfg: &mut GpioConfig) -> Option<Modal> {
        match key.code {
            KeyCode::Left => {
                self.selected = (self.selected + 5) % 6;
                None
            }
            KeyCode::Right => {
                self.selected = (self.selected + 1) % 6;
                None
            }
            KeyCode::Up => {
                self.selected = (self.selected + 3) % 6;
                None
            }
            KeyCode::Down => {
                self.selected = (self.selected + 3) % 6;
                None
            }

            // Delete all mappings for the selected device
            KeyCode::Char('d') | KeyCode::Delete => {
                let device = DEVICE_LIST[self.selected].to_owned();
                Some(Modal::Confirm(ConfirmModal::new(
                    "Delete Device",
                    format!("Delete all mappings for '{device}'?"),
                    move |yes, cfg| {
                        if yes {
                            config::delete_device(cfg, &device);
                            (None, Some(ModalAction::RefreshDevicesTab))
                        } else {
                            (None, None)
                        }
                    },
                )))
            }

            _ => None,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect, cfg: &GpioConfig) {
        let counts = build_count_map(cfg);
        let joypad_counts = build_joypad_count_map(cfg);

        let outer_block = Block::default()
            .title(" Devices [←→↑↓] ")
            .borders(Borders::ALL)
            .border_style(theme::border_normal());
        let inner_area = outer_block.inner(area);
        f.render_widget(outer_block, area);

        let row_areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(inner_area);

        for row_idx in 0..2usize {
            let col_areas = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                    Constraint::Percentage(34),
                ])
                .split(row_areas[row_idx]);

            for col_idx in 0..3usize {
                let device_idx = row_idx * 3 + col_idx;
                let device_name = DEVICE_LIST[device_idx];
                let count = *counts.get(device_name).unwrap_or(&0);
                let is_selected = self.selected == device_idx;

                let border_style = if is_selected {
                    theme::border_focused()
                } else if count > 0 {
                    theme::border_normal()
                } else {
                    Style::default().fg(theme::DIM)
                };

                let title_style = if is_selected {
                    theme::selected_btn()
                } else if count > 0 {
                    Style::default().fg(theme::CYAN)
                } else {
                    Style::default().fg(theme::DIM)
                };

                let block = Block::default()
                    .title(Span::styled(format!(" {} ", device_name), title_style))
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(border_style);

                let cell_style = if is_selected {
                    theme::selected_btn()
                } else if count > 0 {
                    Style::default().fg(theme::CYAN)
                } else {
                    theme::hint_text()
                };

                let lines = if device_name.starts_with("Joypad") {
                    let jc = joypad_counts.get(device_name).copied().unwrap_or((0, 0));
                    device_cell_lines_joypad(jc, cell_style)
                } else {
                    let count_text = match count {
                        0 => "(no mappings)".to_string(),
                        1 => "1 mapping".to_string(),
                        n => format!("{n} mappings"),
                    };
                    vec![Line::from(""), Line::from(Span::styled(count_text, cell_style))]
                };

                let para = Paragraph::new(lines)
                    .alignment(Alignment::Center)
                    .block(block);

                f.render_widget(para, col_areas[col_idx]);
            }
        }
    }
}

impl TabHint for DevicesTab {
    fn hint(&self) -> &str {
        "←→↑↓: navigate  Enter: edit mappings  d: delete device  s: save  q: quit"
    }
}

fn build_count_map(cfg: &GpioConfig) -> HashMap<&'static str, usize> {
    let mut map: HashMap<&'static str, usize> = HashMap::new();
    for row in &cfg.devices {
        if let Some(&key) = DEVICE_LIST.iter().find(|&&d| d == row.device.as_str()) {
            *map.entry(key).or_insert(0) += 1;
        }
    }
    map
}

/// Returns (dpad_groups, button_count) per Joypad device.
/// dpad_groups = distinct "DPAD N" prefixes across AXIS rows.
fn build_joypad_count_map(cfg: &GpioConfig) -> HashMap<&'static str, (usize, usize)> {
    let mut map: HashMap<&'static str, (HashSet<String>, usize)> = HashMap::new();
    for row in &cfg.devices {
        if let Some(&key) = DEVICE_LIST.iter().find(|&&d| d == row.device.as_str()) {
            if !key.starts_with("Joypad") { continue; }
            let entry = map.entry(key).or_insert_with(|| (HashSet::new(), 0));
            if row.event_type == "AXIS" {
                // Name format "DPAD N DIR" — extract first two words as group key
                let group: String = row.name.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
                entry.0.insert(group);
            } else if row.event_type == "BUTTON" {
                entry.1 += 1;
            }
        }
    }
    map.into_iter().map(|(k, (groups, btns))| (k, (groups.len(), btns))).collect()
}

/// Build the info lines for a Joypad cell.
fn device_cell_lines_joypad(counts: (usize, usize), style: ratatui::style::Style) -> Vec<Line<'static>> {
    let (dpads, btns) = counts;
    if dpads == 0 && btns == 0 {
        return vec![
            Line::from(""),
            Line::from(Span::styled("(no mappings)", style)),
        ];
    }
    let mut lines = vec![Line::from("")];
    if dpads > 0 {
        let label = if dpads == 1 { "1 Dpad/joystick".to_string() } else { format!("{dpads} Dpads/joysticks") };
        lines.push(Line::from(Span::styled(label, style)));
    }
    if btns > 0 {
        let label = if btns == 1 { "1 button".to_string() } else { format!("{btns} buttons") };
        lines.push(Line::from(Span::styled(label, style)));
    }
    lines
}
