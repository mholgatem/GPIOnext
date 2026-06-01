use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::{config::GpioConfig, ui::ModalAction};
use super::Modal;

#[derive(Debug, Clone, PartialEq)]
pub enum ChipType { Mcp23017, Ads1115, Pcf8574 }

impl ChipType {
    fn label(&self) -> &'static str {
        match self {
            ChipType::Mcp23017 => "MCP23017 (16-bit GPIO expander)",
            ChipType::Ads1115  => "ADS1115  (4-channel ADC)",
            ChipType::Pcf8574  => "PCF8574  (8-bit GPIO expander)",
        }
    }
}

const CHIP_TYPES: &[ChipType] = &[ChipType::Mcp23017, ChipType::Ads1115, ChipType::Pcf8574];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Step { ChipType, Bus, Address, IntPin }

/// Multi-step modal for adding an I2C chip configuration.
pub struct AddI2cModal {
    step: Step,
    chip_state: ListState,
    chip_type: Option<ChipType>,
    bus: String,
    address: String,
    int_pin: String,
    pub on_confirm: Box<dyn FnOnce(Option<I2cEntry>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>)>,
}

#[derive(Debug, Clone)]
pub struct I2cEntry {
    pub chip: ChipType,
    pub bus: u8,
    pub address: String,
    pub int_pin: String,
}

impl AddI2cModal {
    pub fn new(
        on_confirm: impl FnOnce(Option<I2cEntry>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>) + 'static,
    ) -> Self {
        let mut chip_state = ListState::default();
        chip_state.select(Some(0));
        Self {
            step: Step::ChipType,
            chip_state,
            chip_type: None,
            bus: "1".into(),
            address: "0x20".into(),
            int_pin: String::new(),
            on_confirm: Box::new(on_confirm),
        }
    }

    pub fn handle_key(
        mut self,
        key: KeyEvent,
        cfg: &mut GpioConfig,
    ) -> (Option<Modal>, Option<ModalAction>, bool) {
        match self.step {
            Step::ChipType => match key.code {
                KeyCode::Up => {
                    let i = self.chip_state.selected().unwrap_or(0);
                    if i > 0 { self.chip_state.select(Some(i - 1)); }
                    (Some(Modal::AddI2c(self)), None, false)
                }
                KeyCode::Down => {
                    let i = self.chip_state.selected().unwrap_or(0);
                    if i + 1 < CHIP_TYPES.len() { self.chip_state.select(Some(i + 1)); }
                    (Some(Modal::AddI2c(self)), None, false)
                }
                KeyCode::Enter => {
                    self.chip_type = Some(CHIP_TYPES[self.chip_state.selected().unwrap_or(0)].clone());
                    // Default address by chip type
                    self.address = match self.chip_type.as_ref().unwrap() {
                        ChipType::Ads1115 => "0x48".into(),
                        _ => "0x20".into(),
                    };
                    self.step = Step::Bus;
                    (Some(Modal::AddI2c(self)), None, false)
                }
                KeyCode::Esc => {
                    let (modal, action) = (self.on_confirm)(None, cfg);
                    (modal, action, false)
                }
                _ => (Some(Modal::AddI2c(self)), None, false),
            },

            Step::Bus | Step::Address | Step::IntPin => {
                let field = match self.step {
                    Step::Bus => &mut self.bus,
                    Step::Address => &mut self.address,
                    Step::IntPin => &mut self.int_pin,
                    _ => unreachable!(),
                };
                match key.code {
                    KeyCode::Backspace => { field.pop(); (Some(Modal::AddI2c(self)), None, false) }
                    KeyCode::Char(c) => {
                        field.push(c);
                        (Some(Modal::AddI2c(self)), None, false)
                    }
                    KeyCode::Enter => {
                        match self.step {
                            Step::Bus => { self.step = Step::Address; }
                            Step::Address => {
                                // ADS1115 has no int pin
                                let needs_int = self.chip_type != Some(ChipType::Ads1115);
                                if needs_int {
                                    self.step = Step::IntPin;
                                } else {
                                    return self.finish(cfg);
                                }
                            }
                            Step::IntPin => return self.finish(cfg),
                            _ => unreachable!(),
                        }
                        (Some(Modal::AddI2c(self)), None, false)
                    }
                    KeyCode::Esc => {
                        let (modal, action) = (self.on_confirm)(None, cfg);
                        (modal, action, false)
                    }
                    _ => (Some(Modal::AddI2c(self)), None, false),
                }
            }
        }
    }

    fn finish(self, cfg: &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>, bool) {
        let bus: u8 = self.bus.parse().unwrap_or(1);
        let entry = I2cEntry {
            chip: self.chip_type.clone().unwrap_or(ChipType::Mcp23017),
            bus,
            address: self.address.clone(),
            int_pin: self.int_pin.clone(),
        };
        let (modal, action) = (self.on_confirm)(Some(entry), cfg);
        (modal, action, false)
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let popup = centered_rect(65, 12, area);
        f.render_widget(Clear, popup);

        let title = match self.step {
            Step::ChipType => " Add I2C Device — Select Chip ",
            Step::Bus      => " Add I2C Device — Bus Number ",
            Step::Address  => " Add I2C Device — I2C Address ",
            Step::IntPin   => " Add I2C Device — Interrupt Pin (BOARD, or blank) ",
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(popup);
        f.render_widget(block, popup);

        match self.step {
            Step::ChipType => {
                let items: Vec<ListItem> = CHIP_TYPES
                    .iter()
                    .map(|c| ListItem::new(Line::from(c.label())))
                    .collect();
                let list = List::new(items)
                    .highlight_style(
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol("▶ ");
                f.render_stateful_widget(list, inner, &mut self.chip_state);
            }
            step => {
                let (label, value) = match step {
                    Step::Bus     => ("I2C Bus Number:", &self.bus),
                    Step::Address => ("I2C Address (hex):", &self.address),
                    Step::IntPin  => ("Interrupt BOARD Pin (blank = none):", &self.int_pin),
                    _ => unreachable!(),
                };
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Length(2), Constraint::Min(0)])
                    .split(inner);
                f.render_widget(Paragraph::new(label), chunks[0]);
                f.render_widget(
                    Paragraph::new(format!("> {value}_"))
                        .block(Block::default().borders(Borders::BOTTOM))
                        .style(Style::default().fg(Color::Yellow)),
                    chunks[1],
                );
                f.render_widget(
                    Paragraph::new("Enter to continue  Esc to cancel")
                        .style(Style::default().fg(Color::DarkGray)),
                    chunks[2],
                );
            }
        }
    }
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let w = area.width * percent_x / 100;
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect { x: area.x + x, y: area.y + y, width: w, height }
}
