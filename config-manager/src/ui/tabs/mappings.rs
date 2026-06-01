/// Mappings tab — shows button/key/axis/command rows for the selected device.
/// Provides joypad, keyboard, and command wizards via modal sequences.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::{
    config::{self, DeviceRow, GpioConfig},
    constants::{BUTTON_LIST, COMMAND_PRESETS, DEVICE_LIST, KEY_LIST},
    ui::{
        Modal, ModalAction,
        modals::{
            command_input::CommandInputModal,
            confirm::ConfirmModal,
            pin_capture::PinCaptureModal,
            selection::{MultiSelectModal, SingleSelectModal},
        },
    },
};

pub struct MappingsTab {
    pub selected_device: Option<String>,
    pub state: TableState,
}

impl MappingsTab {
    pub fn new() -> Self {
        Self {
            selected_device: None,
            state: TableState::default(),
        }
    }

    pub fn load_device(&mut self, device: &str, _cfg: &GpioConfig) {
        self.selected_device = Some(device.to_owned());
        self.state.select(Some(0));
    }

    pub fn handle_key(&mut self, key: KeyEvent, cfg: &mut GpioConfig) -> Option<Modal> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let device = self.selected_device.as_ref()?;
                let rows = config::get_device_rows(cfg, device);
                let i = self.state.selected().unwrap_or(0);
                if i > 0 {
                    self.state.select(Some(i - 1));
                }
                let _ = rows; // borrow released
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let device = self.selected_device.as_ref()?;
                let count = config::get_device_rows(cfg, device).len();
                let i = self.state.selected().unwrap_or(0);
                if i + 1 < count {
                    self.state.select(Some(i + 1));
                }
                None
            }

            // Select device first if none selected
            KeyCode::Tab if self.selected_device.is_none() => {
                let devices: Vec<String> = DEVICE_LIST.iter().map(|&s| s.to_owned()).collect();
                Some(Modal::SingleSelect(SingleSelectModal::new(
                    "Select Device",
                    devices,
                    |idx, _cfg| {
                        if let Some(i) = idx {
                            let device = DEVICE_LIST[i].to_owned();
                            (None, Some(ModalAction::RefreshMappingsTab(device)))
                        } else {
                            (None, None)
                        }
                    },
                )))
            }

            // Add mapping wizard — entry point depends on device type
            KeyCode::Char('n') | KeyCode::Char('N') => {
                let device = self.selected_device.clone()?;
                launch_add_wizard(device)
            }

            // Delete selected mapping
            KeyCode::Char('d') | KeyCode::Delete => {
                let device = self.selected_device.as_ref()?;
                let i = self.state.selected()?;
                // Clone the strings we need before dropping the borrow on cfg.
                let name = config::get_device_rows(cfg, device).get(i)?.name.clone();
                let device = device.clone();
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

    pub fn render(&mut self, f: &mut Frame, area: Rect, cfg: &GpioConfig) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0)])
            .split(area);

        // Device selector banner
        let banner = if let Some(ref d) = self.selected_device {
            format!(" Device: {d}  [n] Add  [d] Delete  [Tab] Switch device ")
        } else {
            " No device selected — press Tab to choose ".to_owned()
        };
        f.render_widget(
            Paragraph::new(Line::from(banner.as_str()))
                .block(Block::default().borders(Borders::BOTTOM))
                .style(Style::default().fg(Color::Cyan)),
            chunks[0],
        );

        let device = match &self.selected_device {
            Some(d) => d.clone(),
            None => {
                f.render_widget(
                    Paragraph::new("Press [Tab] to select a device."),
                    chunks[1],
                );
                return;
            }
        };

        let rows_data = config::get_device_rows(cfg, &device);

        let header = Row::new(vec![
            Cell::from("Name").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Type").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Pins").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Command / Code").style(Style::default().add_modifier(Modifier::BOLD)),
        ])
        .style(Style::default().fg(Color::Yellow));

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
        .block(Block::default().borders(Borders::ALL).title(" Mappings "))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

        f.render_stateful_widget(table, chunks[1], &mut self.state);
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

// --- Joypad wizard: AXIS or BUTTON selection, then pin capture ---

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
    // For an axis, pick direction
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
                                let pins_str = if vpins.len() == 1 {
                                    vpins[0].to_string()
                                } else {
                                    format!("({})", vpins.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", "))
                                };
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
                                let pins_str = if vpins.len() == 1 {
                                    vpins[0].to_string()
                                } else {
                                    format!("({})", vpins.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", "))
                                };
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

// --- Keyboard wizard ---

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
                                let pins_str = if vpins.len() == 1 {
                                    vpins[0].to_string()
                                } else {
                                    format!("({})", vpins.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", "))
                                };
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

// --- Command wizard: optionally pick a preset or type custom ---

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
                // Custom: open command input modal
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
