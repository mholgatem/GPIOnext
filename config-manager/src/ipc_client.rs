/// ipc_client.rs — Unix socket client for live GPIO pin state.
///
/// Connects to the running gpionext daemon's IPC socket and parses
/// newline-delimited JSON frames into a shared PinState. The config manager
/// reads PinState on every render tick for the live pin view and pin capture
/// modals without blocking the UI thread.
///
/// If the daemon is not running the socket won't exist; the client retries
/// every 2 seconds in the background and the UI shows a "disconnected" badge.

use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::UnixStream;

/// Snapshot of all pin states received from the daemon.
#[derive(Default, Clone)]
pub struct PinState {
    /// Physical BOARD pin states indexed by virtual pin number (0-255).
    /// `true` = pressed/high.
    pub pins: Vec<bool>,
    /// Set to true when the IPC socket is connected and streaming.
    pub connected: bool,
}

impl PinState {
    pub fn is_pressed(&self, vpin: u8) -> bool {
        self.pins.get(vpin as usize).copied().unwrap_or(false)
    }

    /// Return the list of currently pressed virtual pin numbers.
    pub fn pressed_vpins(&self) -> Vec<u8> {
        self.pins
            .iter()
            .enumerate()
            .filter_map(|(i, &v)| if v { Some(i as u8) } else { None })
            .collect()
    }
}

pub const SOCKET_PATH: &str = "/tmp/gpionext.sock";

pub struct IpcClient;

impl IpcClient {
    /// Spawn a background thread that connects to the daemon socket and
    /// continuously updates `state`. Reconnects automatically on disconnect.
    pub fn start(state: Arc<Mutex<PinState>>) {
        std::thread::Builder::new()
            .name("gpionext-ipc".into())
            .spawn(move || {
                ipc_loop(state);
            })
            .expect("spawn ipc thread");
    }
}

fn ipc_loop(state: Arc<Mutex<PinState>>) {
    loop {
        match try_connect(&state) {
            Ok(()) => {}
            Err(_) => {
                // Mark disconnected, wait before retry
                if let Ok(mut s) = state.lock() {
                    s.connected = false;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

#[cfg(unix)]
fn try_connect(state: &Arc<Mutex<PinState>>) -> anyhow::Result<()> {
    let stream = UnixStream::connect(SOCKET_PATH)?;
    {
        let mut s = state.lock().unwrap();
        s.connected = true;
    }
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line?;
        if let Ok(frame) = serde_json::from_str::<PinFrame>(&line) {
            if let Ok(mut s) = state.lock() {
                s.pins = frame.pins;
                s.connected = true;
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn try_connect(_state: &Arc<Mutex<PinState>>) -> anyhow::Result<()> {
    // Not supported on non-Unix platforms (e.g. Windows dev machine).
    // Sleep so the loop doesn't spin hot.
    std::thread::sleep(Duration::from_secs(10));
    Ok(())
}

#[derive(serde::Deserialize)]
struct PinFrame {
    pins: Vec<bool>,
}
