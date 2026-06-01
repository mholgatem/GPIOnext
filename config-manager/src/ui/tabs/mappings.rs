/// Mappings tab — shows button/key/axis/command rows for all or one device.
///
/// Default: shows all devices' mappings with a "Device" column.
/// Press f/c to filter to a single device; "All" clears the filter.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::{
    config::{self, DeviceRow, GpioConfig},
    constants::{BUTTON_LIST, COMMAND_PRESETS, DEVICE_LIST, KEY_LIST},
    ui::{
        modals::{
            command_input::CommandInputModal,
            confirm::ConfirmModal,
            pin_capture::PinCaptureModal,
            selection::SingleSelectModal,
            Modal,
        },
        theme, ModalAction,
    },
};
use super::TabHint;

pub struct MappingsTab {
    /// None = show all devices; Some(name) = show only that device.
    pub filter_device: Option<String>,
    pub state: TableState,
}

impl MappingsTab {
    pub fn new() -> Self {
        Self {
            filter_device: None,
            state: TableState::default(),
        }
    }

    /// Set the active device filter and reset the cursor.
    pub fn set_filter(&mut self, device: Option<String>) {
        self.filter_device = device;
        self.state.select(Some(0));
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
                let count = self.visible_row_count(cfg);
                let i = self.state.selected().unwrap_or(0);
                if i + 1 < count {
                    self.state.select(Some(i + 1));
                }
                None
            }

            // f / c: open filter modal (All + each device)
            KeyCode::Char('f') | KeyCode::Char('F') | KeyCode::Char('c') | KeyCode::Char('C') => {
                let mut items = vec!["All".to_owned()];
                items.extend(DEVICE_LIST.iter().map(|&s| s.to_owned()));
                Some(Modal::SingleSelect(SingleSelectModal::new(
                    "Filter by Device",
                    items,
                    |idx, _cfg| match idx {
                        None => (None, None), // Esc: no change
                        Some(0) => (None, Some(ModalAction::SetMappingsFilter(None))), // All
                        Some(i) => (
                            None,
                            Some(ModalAction::SetMappingsFilter(Some(
                                DEVICE_LIST[i - 1].to_owned(),
                            ))),
                        ),
                    },
                )))
            }

            // n: add mapping. If filtered, go straight to wizard; otherwise pick device first.
            KeyCode::Char('n') | KeyCode::Char('N') => {
                if let Some(ref device) = self.filter_device.clone() {
                    launch_add_wizard(device.clone())
                } else {
                    let devices: Vec<String> = DEVICE_LIST.iter().map(|&s| s.to_owned()).collect();
                    Some(Modal::SingleSelect(SingleSelectModal::new(
                        "Select Device for New Mapping",
                        devices,
                        |idx, _cfg| {
                            if let Some(i) = idx {
                                let device = DEVICE_LIST[i].to_owned();
                                (launch_add_wizard(device), None)
                            } else {
                                (None, None)
                            }
                        },
                    )))
                }
            }

            // d: delete selected mapping
            KeyCode::Char('d') | KeyCode::Delete => {
                let i = self.state.selected()?;
                let (name, device) = if let Some(ref dev) = self.filter_device {
                    let rows = config::get_device_rows(cfg, dev);
                    let row = rows.get(i)?;
                    (row.name.clone(), dev.clone())
                } else {
                    let row = cfg.devices.get(i)?;
                    (row.name.clone(), row.device.clone())
                };
                Some(Modal::Confirm(ConfirmModal::new(
                    "Delete Mapping",
                    format!("Delete '{name}' from {device}?"),
                    move |yes, cfg| {
                        if yes {
                            config::delete_mapping(cfg, &device, &name);
                            (None, Some(ModalAction::Save))
                        } else {
                            (None, None)
                        }
                    },
                )))
            }

            _ => None,
        }
    }

    fn visible_row_count(&self, cfg: &GpioConfig) -> usize {
        if let Some(ref dev) = self.filter_device {
            config::get_device_rows(cfg, dev).len()
        } else {
            cfg.devices.len()
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, cfg: &GpioConfig) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0)])
            .split(area);

        // Banner / header
        let banner = if let Some(ref d) = self.filter_device {
            format!(" Device: {d}  [n] Add  [d] Delete  [f] Filter  [c] Change ")
        } else {
            " All Devices  [n] Add  [d] Delete  [f] Filter by device ".to_owned()
        };
        f.render_widget(
            Paragraph::new(Line::from(banner.as_str()))
                .block(
                    Block::default()
                        .borders(Borders::BOTTOM)
                        .border_style(theme::border_normal()),
                )
                .style(Style::default().fg(theme::CYAN)),
            chunks[0],
        );

        if let Some(ref device) = self.filter_device.clone() {
            self.render_filtered(f, chunks[1], cfg, device);
        } else {
            self.render_all(f, chunks[1], cfg);
        }
    }

    fn render_all(&mut self, f: &mut Frame, area: Rect, cfg: &GpioConfig) {
        if cfg.devices.is_empty() {
            f.render_widget(
                Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "  No mappings configured yet.",
                        theme::hint_text(),
                    )),
                    Line::from(Span::styled(
                        "  Press [n] to add, or [f] to filter by device.",
                        Style::default().fg(theme::CYAN),
                    )),
                ])
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(theme::border_normal())
                        .title(" All Mappings "),
                ),
                area,
            );
            return;
        }

        let header = Row::new(vec![
            Cell::from("Device").style(theme::header()),
            Cell::from("Name").style(theme::header()),
            Cell::from("Type").style(theme::header()),
            Cell::from("Pins").style(theme::header()),
            Cell::from("Command / Code").style(theme::header()),
        ]);

        let rows: Vec<Row> = cfg
            .devices
            .iter()
            .map(|r| {
                Row::new(vec![
                    Cell::from(r.device.as_str()),
                    Cell::from(r.name.as_str()),
                    Cell::from(r.event_type.as_str()),
                    Cell::from(r.pins.as_str()),
                    Cell::from(r.command.as_str()),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(18),
                Constraint::Percentage(22),
                Constraint::Percentage(10),
                Constraint::Percentage(18),
                Constraint::Percentage(32),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border_normal())
                .title(" All Mappings "),
        )
        .highlight_style(theme::selected_row())
        .highlight_symbol("▶ ");

        f.render_stateful_widget(table, area, &mut self.state);
    }

    fn render_filtered(&mut self, f: &mut Frame, area: Rect, cfg: &GpioConfig, device: &str) {
        let rows_data = config::get_device_rows(cfg, device);

        if rows_data.is_empty() {
            f.render_widget(
                Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        format!("  No mappings for {device} yet."),
                        theme::hint_text(),
                    )),
                    Line::from(Span::styled(
                        "  Press [n] to add a mapping.",
                        Style::default().fg(theme::CYAN),
                    )),
                ])
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(theme::border_normal())
                        .title(format!(" {device} Mappings ")),
                ),
                area,
            );
            return;
        }

        let header = Row::new(vec![
            Cell::from("Name").style(theme::header()),
            Cell::from("Type").style(theme::header()),
            Cell::from("Pins").style(theme::header()),
            Cell::from("Command / Code").style(theme::header()),
        ]);

        let rows: Vec<Row> = rows_data
            .iter()
            .map(|r| {
                Row::new(vec![
                    Cell::from(r.name.as_str()),
                    Cell::from(r.event_type.as_str()),
                    Cell::from(r.pins.as_str()),
                    Cell::from(r.command.as_str()),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(30),
                Constraint::Percentage(12),
                Constraint::Percentage(20),
                Constraint::Percentage(38),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border_normal())
                .title(format!(" {device} Mappings ")),
        )
        .highlight_style(theme::selected_row())
        .highlight_symbol("▶ ");

        f.render_stateful_widget(table, area, &mut self.state);
    }
}

impl TabHint for MappingsTab {
    fn hint(&self) -> &str {
        "↑↓/jk: move  n: add  d: delete  f/c: filter device  s: save  q: quit"
    }
}

// ---------------------------------------------------------------------------
// Wizard launchers — return the first modal in each wizard chain
// ---------------------------------------------------------------------------

fn launch_add_wizard(device: String) -> Option<Modal> {
    if device.starts_with("Joypad") {
        Some(joypad_type_modal(device))
    } else if device == "Keyboard" {
        Some(keyboard_wizard_modal(device))
    } else if device == "Commands" {
        Some(command_wizard_modal(device))
    } else {
        None
    }
}

fn joypad_type_modal(device: String) -> Modal {
    Modal::SingleSelect(SingleSelectModal::new(
        "Add Joypad Mapping — Type",
        vec!["Axis (D-Pad direction)".into(), "Button".into()],
        move |idx, _cfg| match idx {
            Some(0) => (Some(joypad_axis_name_modal(device)), None),
            Some(1) => (Some(joypad_button_modal(device)), None),
            _ => (None, None),
        },
    ))
}

fn joypad_axis_name_modal(device: String) -> Modal {
    Modal::SingleSelect(SingleSelectModal::new(
        "Add Axis — Direction",
        vec!["UP".into(), "DOWN".into(), "LEFT".into(), "RIGHT".into()],
        move |idx, _cfg| {
            if let Some(i) = idx {
                let (direction, axis_code, value) = match i {
                    0 => ("UP",    1i32, -255i32),
                    1 => ("DOWN",  1,     255),
                    2 => ("LEFT",  0,    -255),
                    _ => ("RIGHT", 0,     255),
                };
                let command = format!("(3, {axis_code}, {value})");
                let name = format!("DPAD 1 {direction}");
                let dev2 = device.clone();
                (
                    Some(Modal::PinCapture(PinCaptureModal::new(
                        format!("Hold pin for {name}"),
                        move |pins, cfg| {
                            if let Some(vpins) = pins {
                                let pins_str = pins_to_str(&vpins);
                                config::upsert_mapping(cfg, DeviceRow::new(&dev2, &name, "AXIS", &command, pins_str));
                            }
                            (None, Some(ModalAction::Save))
                        },
                    ))),
                    None,
                )
            } else {
                (None, None)
            }
        },
    ))
}

fn joypad_button_modal(device: String) -> Modal {
    let items: Vec<String> = BUTTON_LIST.iter().map(|&(name, _)| name.to_owned()).collect();
    Modal::SingleSelect(SingleSelectModal::new(
        "Add Button — Select Type",
        items,
        move |idx, _cfg| {
            if let Some(i) = idx {
                let (btn_name, evdev) = BUTTON_LIST[i];
                let name = btn_name.to_owned();
                let code = evdev.to_string();
                let dev2 = device.clone();
                (
                    Some(Modal::PinCapture(PinCaptureModal::new(
                        format!("Hold pin for {name}"),
                        move |pins, cfg| {
                            if let Some(vpins) = pins {
                                let pins_str = pins_to_str(&vpins);
                                config::upsert_mapping(cfg, DeviceRow::new(&dev2, &name, "BUTTON", &code, pins_str));
                            }
                            (None, Some(ModalAction::Save))
                        },
                    ))),
                    None,
                )
            } else {
                (None, None)
            }
        },
    ))
}

fn keyboard_wizard_modal(device: String) -> Modal {
    let items: Vec<String> = KEY_LIST.iter().map(|&(name, _)| name.to_owned()).collect();
    Modal::SingleSelect(SingleSelectModal::new(
        "Add Key — Select Key",
        items,
        move |idx, _cfg| {
            if let Some(i) = idx {
                let (key_name, evdev) = KEY_LIST[i];
                let name = key_name.to_owned();
                let code = evdev.to_string();
                let dev2 = device.clone();
                (
                    Some(Modal::PinCapture(PinCaptureModal::new(
                        format!("Hold pin for {name}"),
                        move |pins, cfg| {
                            if let Some(vpins) = pins {
                                let pins_str = pins_to_str(&vpins);
                                config::upsert_mapping(cfg, DeviceRow::new(&dev2, &name, "KEY", &code, pins_str));
                            }
                            (None, Some(ModalAction::Save))
                        },
                    ))),
                    None,
                )
            } else {
                (None, None)
            }
        },
    ))
}

fn command_wizard_modal(device: String) -> Modal {
    let mut items: Vec<String> = COMMAND_PRESETS.iter().map(|&(name, _)| name.to_owned()).collect();
    items.push("Custom command...".into());

    Modal::SingleSelect(SingleSelectModal::new(
        "Add Command — Choose",
        items,
        move |idx, _cfg| {
            if let Some(i) = idx {
                if i < COMMAND_PRESETS.len() {
                    let (preset_name, preset_cmd) = COMMAND_PRESETS[i];
                    let name = preset_name.to_owned();
                    let cmd = preset_cmd.to_owned();
                    let dev2 = device.clone();
                    return (
                        Some(Modal::PinCapture(PinCaptureModal::new(
                            format!("Hold pin for '{name}'"),
                            move |pins, cfg| {
                                if let Some(vpins) = pins {
                                    let pins_str = vpins[0].to_string();
                                    config::upsert_mapping(cfg, DeviceRow::new(&dev2, &name, "COMMAND", &cmd, pins_str));
                                }
                                (None, Some(ModalAction::Save))
                            },
                        ))),
                        None,
                    );
                }
                let dev2 = device.clone();
                return (
                    Some(Modal::CommandInput(CommandInputModal::new(
                        "Enter Command",
                        move |result, _cfg| {
                            if let Some((cmd, _timeout)) = result {
                                let dev3 = dev2.clone();
                                let cmd2 = cmd.clone();
                                return (
                                    Some(Modal::PinCapture(PinCaptureModal::new(
                                        "Hold pin for command",
                                        move |pins, cfg| {
                                            if let Some(vpins) = pins {
                                                let pins_str = vpins[0].to_string();
                                                config::upsert_mapping(cfg, DeviceRow::new(
                                                    &dev3, &cmd2, "COMMAND", &cmd2, pins_str,
                                                ));
                                            }
                                            (None, Some(ModalAction::Save))
                                        },
                                    ))),
                                    None,
                                );
                            }
                            (None, None)
                        },
                    ))),
                    None,
                );
            }
            (None, None)
        },
    ))
}

fn pins_to_str(vpins: &[u8]) -> String {
    if vpins.len() == 1 {
        vpins[0].to_string()
    } else {
        format!("({})", vpins.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", "))
    }
}
