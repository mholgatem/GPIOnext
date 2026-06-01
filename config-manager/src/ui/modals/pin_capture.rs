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

use crate::{config::GpioConfig, ipc_client::PinState, ui::ModalAction};
use super::Modal;

const HOLD_DURATION: Duration = Duration::from_millis(1000);

/// Modal that waits for the user to hold one or more GPIO pins for 1 second.
/// Polls PinState on every render tick (called every 50ms from the event loop).
pub struct PinCaptureModal {
    pub title: String,
    /// Pins held at the start of the hold window (None = waiting for first press)
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
        if key.code == KeyCode::Esc {
            let (modal, action) = (self.on_capture)(None, cfg);
            return (modal, action, false);
        }
        (Some(Modal::PinCapture(self)), None, false)
    }

    /// Advance the hold timer based on current pin state.
    /// App calls this on every 50ms tick, then checks `hold_complete()`.
    pub fn tick(&mut self, pin_state: &Arc<Mutex<PinState>>) {
        let pressed = {
            let s = pin_state.lock().unwrap();
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
            Some(ref prev) if prev != &pressed => {
                // Pin set changed — restart window
                self.hold_pins = Some(pressed);
                self.hold_since = Some(Instant::now());
            }
            _ => {} // same pins, timer running — check hold_complete()
        }
    }

    /// True when the hold has been sustained for HOLD_DURATION.
    pub fn hold_complete(&self) -> bool {
        self.hold_since
            .map(|s| s.elapsed() >= HOLD_DURATION)
            .unwrap_or(false)
    }

    pub fn render(&self, f: &mut Frame, area: Rect, pin_state: &Arc<Mutex<PinState>>) {
        let popup = centered_rect(60, 10, area);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta));

        let inner = block.inner(popup);
        f.render_widget(block, popup);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // status line
                Constraint::Length(1), // pins line
                Constraint::Length(1), // hold bar
                Constraint::Min(0),    // hint
            ])
            .split(inner);

        let (connected, pressed) = {
            let s = pin_state.lock().unwrap();
            (s.connected, s.pressed_vpins())
        };

        let status_span = if connected {
            Span::styled("● Connected", Style::default().fg(Color::Green))
        } else {
            Span::styled("○ Daemon not running", Style::default().fg(Color::Red))
        };
        f.render_widget(
            Paragraph::new(Line::from(status_span)).alignment(Alignment::Center),
            chunks[0],
        );

        let pin_label = if pressed.is_empty() {
            "Hold pin(s) to capture...".to_owned()
        } else {
            format!("Holding: {:?}", pressed)
        };
        f.render_widget(
            Paragraph::new(Line::from(pin_label)).alignment(Alignment::Center),
            chunks[1],
        );

        // Progress bar for hold duration
        let ratio = self
            .hold_since
            .map(|s| (s.elapsed().as_millis() as f64 / HOLD_DURATION.as_millis() as f64).min(1.0))
            .unwrap_or(0.0);
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .ratio(ratio);
        f.render_widget(gauge, chunks[2]);

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Esc to cancel",
                Style::default().fg(Color::DarkGray),
            )))
            .alignment(Alignment::Center),
            chunks[3],
        );
    }
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let w = area.width * percent_x / 100;
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect { x: area.x + x, y: area.y + y, width: w, height }
}
