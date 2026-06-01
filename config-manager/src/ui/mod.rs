pub mod modals;
pub mod tabs;
pub mod live_pin_view;

use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, Frame};
use std::sync::{Arc, Mutex};

use crate::{
    config::GpioConfig,
    ipc_client::PinState,
    init_sys::DaemonCmd,
};

pub use modals::Modal;

/// Actions a modal can return to the App after it closes.
#[derive(Debug)]
pub enum ModalAction {
    /// Persist the current config to disk immediately.
    Save,
    /// Display a transient status message in the bottom bar.
    StatusMsg(String),
    /// Rebuild the Devices tab from the updated config.
    RefreshDevicesTab,
    /// Rebuild the Mappings tab for the given device.
    RefreshMappingsTab(String),
    /// Send a lifecycle command to the daemon.
    DaemonAction(DaemonCmd),
}
