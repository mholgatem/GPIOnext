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

/// Modal for capturing GPIO pin(s) by physically holding them for 1 second.
///
/// Requires a connected daemon. Esc skips (calls on_capture with None).
pub struct PinCaptureModal {
    pub title: String,
    pub hold_pins: Option<Vec<u8>>,
    hold_since: Option<Instant>,
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
            on_capture: Box::new(on_capture),
        }
    }

    pub fn handle_key(
        self,
        key: KeyEvent,
        cfg: &mut GpioConfig,
    ) -> (Option<Modal>, Option<ModalAction>, bool) {
        match key.code {
            KeyCode::Esc => {
                let (modal, action) = (self.on_capture)(None, cfg);
                (modal, action, false)
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
        let popup = centered_rect(62, 9, area);
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
                Constraint::Length(1), // separator
                Constraint::Length(1), // live pins
                Constraint::Length(1), // hold gauge
                Constraint::Min(0),    // esc hint
            ])
            .split(inner);

        // ── Daemon status ────────────────────────────────────────────────────
        let (status_text, status_color) = if connected {
            ("● Daemon connected — hold pin(s) for 1 second to capture", Color::LightGreen)
        } else {
            ("○ Daemon not running", Color::Yellow)
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

        // ── Esc hint ─────────────────────────────────────────────────────────
        f.render_widget(
            Paragraph::new(Line::from(Span::styled("Esc: skip", theme::hint_text())))
                .alignment(Alignment::Center),
            chunks[4],
        );
    }
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let w = area.width * percent_x / 100;
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect { x: area.x + x, y: area.y + y, width: w, height }
}
