pub mod live_pin_view;
pub mod modals;
pub mod tabs;

pub use modals::Modal;

use crate::init_sys::DaemonCmd;

/// Actions a modal can return to the App after it closes.
#[derive(Debug)]
pub enum ModalAction {
    /// Persist config to disk.
    Save,
    /// Show a transient message in the status bar.
    StatusMsg(String),
    /// Rebuild the Devices tab row list.
    RefreshDevicesTab,
    /// Load `device` in the Mappings tab and switch to it.
    RefreshMappingsTab(String),
    /// Send a lifecycle command to the daemon.
    DaemonAction(DaemonCmd),
}
