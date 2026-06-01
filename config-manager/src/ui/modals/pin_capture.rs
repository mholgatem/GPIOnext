use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph},
    Frame,
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{config::GpioConfig, ipc_client::PinState, ui::{theme, ModalAction}};
use super::Modal;

const HOLD_DURATION: Duration = Duration::from_millis(1000);

/// Modal for capturing GPIO pin(s).
///
/// Two modes depending on daemon availability:
/// - **Live** (daemon connected): user physically holds pin(s) for 1 second.
/// - **Manual** (daemon not running): user types BOARD pin numbers directly.
///
/// Both modes are always available simultaneously — typing always works.
pub struct PinCaptureModal {
    pub title: String,
    /// Pins held at the start of the live hold window
    pub hold_pins: Option<Vec<u8>>,
    hold_since: Option<Instant>,
    /// Manual text entry buffer ("11" or "11,13" for combos)
    input: String,
    pub on_capture: Box<dyn FnOnce(Option<Vec<u8>>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>)>,
}

impl PinCaptureModal {
    pub fn new(
        title: impl Into<String>,
        on_capture: impl FnOnce(Option<Vec<u8>>, &mut GpioConfig) -> (Option<Modal>, Option<ModalAction>) + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            hold_pins: None,
            hold_since: None,
            input: String::new(),
            on_capture: Box::new(on_capture),
        }
    }

    pub fn handle_key(
        mut self,
        key: KeyEvent,
        cfg: &mut GpioConfig,
    ) -> (Option<Modal>, Option<ModalAction>, bool) {
        match key.code {
            KeyCode::Esc => {
                let (modal, action) = (self.on_capture)(None, cfg);
                (modal, action, false)
            }
            KeyCode::Enter => {
                let pins = parse_pin_input(&self.input);
                if pins.is_empty() {
                    return (Some(Modal::PinCapture(self)), None, false);
                }
                let (modal, action) = (self.on_capture)(Some(pins), cfg);
                (modal, action, false)
            }
            // Handle all backspace variants
            KeyCode::Backspace | KeyCode::Char('\x08') | KeyCode::Char('\x7f') => {
                self.input.pop();
                (Some(Modal::PinCapture(self)), None, false)
            }
            KeyCode::Char(c) if c.is_ascii_digit() || c == ',' || c == ' ' => {
                self.input.push(c);
                (Some(Modal::PinCapture(self)), None, false)
            }
            _ => (Some(Modal::PinCapture(self)), None, false),
        }
    }

    /// Advance the live hold timer. Called every 50 ms by App::tick.
    pub fn tick(&mut self, pin_state: &Arc<Mutex<PinState>>) {
        let pressed = {
            let s = pin_state.lock().unwrap();
            if !s.connected {
                return;
            }
            s.pressed_vpins()
        };

        if pressed.is_empty() {
            self.hold_pins = None;
            self.hold_since = None;
            return;
        }

        match &self.hold_pins {
            None => {
                self.hold_pins = Some(pressed);
                self.hold_since = Some(Instant::now());
            }
            Some(prev) if *prev != pressed => {
                self.hold_pins = Some(pressed);
                self.hold_since = Some(Instant::now());
            }
            _ => {}
        }
    }

    /// True when the physical hold has been sustained for HOLD_DURATION.
    pub fn hold_complete(&self) -> bool {
        self.hold_since
            .map(|s| s.elapsed() >= HOLD_DURATION)
            .unwrap_or(false)
    }

    pub fn render(&self, f: &mut Frame, area: Rect, pin_state: &Arc<Mutex<PinState>>) {
        let popup = centered_rect(62, 14, area);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(theme::border_focused());

        let inner = block.inner(popup);
        f.render_widget(block, popup);

        let (connected, pressed) = {
            let s = pin_state.lock().unwrap();
            (s.connected, s.pressed_vpins())
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // daemon status
                Constraint::Length(1), // separator / divider
                Constraint::Length(1), // live pins
                Constraint::Length(1), // hold bar
                Constraint::Length(1), // separator
                Constraint::Length(1), // manual entry label
                Constraint::Length(1), // manual entry field
                Constraint::Min(0),    // hints
            ])
            .split(inner);

        // ── Daemon status ────────────────────────────────────────────────────
        let (status_text, status_color) = if connected {
            ("● Daemon connected — hold pin(s) for 1 second to capture", Color::LightGreen)
        } else {
            ("○ Daemon not running — use manual entry below", Color::Yellow)
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(status_text, Style::default().fg(status_color))))
                .alignment(Alignment::Center),
            chunks[0],
        );

        // ── Live hold section ────────────────────────────────────────────────
        let pin_label = if !connected {
            Span::styled("(connect daemon for live pin capture)", theme::hint_text())
        } else if pressed.is_empty() {
            Span::styled("Hold a pin now…", theme::hint_text())
        } else {
            Span::styled(
                format!("Holding BOARD pin(s): {:?}", pressed),
                Style::default().fg(theme::CYAN).add_modifier(Modifier::BOLD),
            )
        };
        f.render_widget(
            Paragraph::new(Line::from(pin_label)).alignment(Alignment::Center),
            chunks[2],
        );

        let ratio = if connected {
            self.hold_since
                .map(|s| (s.elapsed().as_millis() as f64 / HOLD_DURATION.as_millis() as f64).min(1.0))
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let gauge_style = if connected {
            Style::default().fg(theme::MAGENTA).add_modifier(Modifier::BOLD)
        } else {
            theme::hint_text()
        };
        f.render_widget(Gauge::default().gauge_style(gauge_style).ratio(ratio), chunks[3]);

        // Divider
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "─────────────────────────────────────────────",
                theme::hint_text(),
            )))
            .alignment(Alignment::Center),
            chunks[4],
        );

        // ── Manual entry ─────────────────────────────────────────────────────
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Enter BOARD pin number(s) — comma-separated for combos:",
                Style::default().fg(Color::White),
            ))),
            chunks[5],
        );

        let input_display = format!(" > {}▋", self.input);
        let input_style = if self.input.is_empty() {
            theme::hint_text()
        } else {
            theme::input_text()
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(input_display, input_style)))
                .block(Block::default().borders(Borders::BOTTOM).border_style(theme::border_normal())),
            chunks[6],
        );

        // Hints
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Enter: confirm   Esc: cancel   (e.g. 11  or  11,13 for combo)",
                theme::hint_text(),
            )))
            .alignment(Alignment::Center),
            chunks[7],
        );
    }
}

/// Parse "11" → [11]  or  "11,13" → [11,13]
fn parse_pin_input(s: &str) -> Vec<u8> {
    s.split(',')
        .filter_map(|t| t.trim().parse::<u8>().ok())
        .collect()
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let w = area.width * percent_x / 100;
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect { x: area.x + x, y: area.y + y, width: w, height }
}
