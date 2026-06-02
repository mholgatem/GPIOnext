pub mod confirm;
pub mod pin_capture;
pub mod selection;
pub mod command_input;
pub mod add_i2c;
pub mod text_input;

use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, Frame};
use std::sync::{Arc, Mutex};

use crate::{config::GpioConfig, ipc_client::PinState, ui::ModalAction};

/// Variant enum wrapping every modal type.
pub enum Modal {
    Confirm(confirm::ConfirmModal),
    PinCapture(pin_capture::PinCaptureModal),
    SingleSelect(selection::SingleSelectModal),
    MultiSelect(selection::MultiSelectModal),
    CommandInput(command_input::CommandInputModal),
    AddI2c(add_i2c::AddI2cModal),
    TextInput(text_input::TextInputModal),
}

impl Modal {
    /// Handle a key event. Returns (next_modal, action, quit_flag).
    pub fn handle_key(
        self,
        key: KeyEvent,
        cfg: &mut GpioConfig,
    ) -> (Option<Modal>, Option<ModalAction>, bool) {
        match self {
            Modal::Confirm(m) => m.handle_key(key, cfg),
            Modal::PinCapture(m) => m.handle_key(key, cfg),
            Modal::SingleSelect(m) => m.handle_key(key, cfg),
            Modal::MultiSelect(m) => m.handle_key(key, cfg),
            Modal::CommandInput(m) => m.handle_key(key, cfg),
            Modal::AddI2c(m) => m.handle_key(key, cfg),
            Modal::TextInput(m) => m.handle_key(key, cfg),
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, pin_state: &Arc<Mutex<PinState>>) {
        match self {
            Modal::Confirm(m) => m.render(f, area),
            Modal::PinCapture(m) => m.render(f, area, pin_state),
            Modal::SingleSelect(m) => m.render(f, area),
            Modal::MultiSelect(m) => m.render(f, area),
            Modal::CommandInput(m) => m.render(f, area),
            Modal::AddI2c(m) => m.render(f, area),
            Modal::TextInput(m) => m.render(f, area),
        }
    }
}
