/// ipc.rs — Unix socket IPC for the GPIOnext daemon.
///
/// Two sockets (Unix-only):
///
/// 1. `/tmp/gpionext.sock` — **pin broadcast** (output only)
///    Pushes `{"pins":[bool,...]}\n` to every connected client every 50 ms.
///    Read by `gpionext-config`'s live pin view.
///
/// 2. `/tmp/gpionext-cmd.sock` — **command socket** (request / response)
///    Each connection sends one JSON command line, receives one JSON response,
///    then disconnects.
///
/// `DaemonCmd` and `CmdRequest` are defined on all platforms so the daemon
/// crate can reference them unconditionally; the socket servers are
/// `#[cfg(unix)]` only.

// mpsc is needed for CmdRequest on all platforms.
use std::sync::mpsc;

pub const SOCKET_PATH:     &str = "/tmp/gpionext.sock";
pub const CMD_SOCKET_PATH: &str = "/tmp/gpionext-cmd.sock";

// ---------------------------------------------------------------------------
// DaemonCmd — commands understood by the native daemon (all platforms)
// ---------------------------------------------------------------------------

/// Commands that can be sent to the running daemon via the command socket.
#[derive(Debug)]
pub enum DaemonCmd {
    Reload,
    Stop,
    Status,
    SetComboDelay(u32),
    SetDebounce(u32),
    /// Replace the active BOARD pin list and restart the GPIO loop.
    SetPins(Vec<u8>),
    /// Enable or disable I2C drivers and restart accordingly.
    UseI2c(bool),
}

/// A command paired with a reply channel.
pub type CmdRequest = (DaemonCmd, mpsc::SyncSender<String>);

// ---------------------------------------------------------------------------
// Unix-only socket implementation
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod unix_impl {
    use super::*;

    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread,
        time::Duration,
    };

    // ── Pin broadcast ──────────────────────────────────────────────────────

    pub fn start_ipc_server(running: Arc<AtomicBool>) {
        let _ = std::fs::remove_file(super::SOCKET_PATH);

        thread::Builder::new()
            .name("gpionext-ipc".into())
            .spawn(move || ipc_server_loop(running))
            .expect("spawn ipc server thread");
    }

    fn ipc_server_loop(running: Arc<AtomicBool>) {
        let listener = match UnixListener::bind(super::SOCKET_PATH) {
            Ok(l) => l,
            Err(e) => { eprintln!("[gpionext-ipc] bind failed: {e}"); return; }
        };
        listener.set_nonblocking(true).expect("set_nonblocking");

        let clients: Arc<Mutex<Vec<std::os::unix::net::UnixStream>>> =
            Arc::new(Mutex::new(Vec::new()));

        {
            let clients2 = Arc::clone(&clients);
            let running2 = Arc::clone(&running);
            thread::Builder::new()
                .name("gpionext-ipc-bcast".into())
                .spawn(move || broadcast_loop(clients2, running2))
                .expect("spawn ipc broadcast thread");
        }

        while running.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Ok(mut c) = clients.lock() { c.push(stream); }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    eprintln!("[gpionext-ipc] accept error: {e}");
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }

        let _ = std::fs::remove_file(super::SOCKET_PATH);
    }

    fn broadcast_loop(
        clients: Arc<Mutex<Vec<std::os::unix::net::UnixStream>>>,
        running: Arc<AtomicBool>,
    ) {
        while running.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(50));

            let words = crate::bitmask::current_bitmask();
            let pins: Vec<bool> = (0..256usize)
                .map(|i| (words[i / 64] >> (i % 64)) & 1 == 1)
                .collect();

            let frame = match serde_json::to_string(&PinFrame { pins }) {
                Ok(s) => format!("{s}\n"),
                Err(_) => continue,
            };
            let bytes = frame.as_bytes();

            if let Ok(mut guard) = clients.lock() {
                guard.retain_mut(|stream| stream.write_all(bytes).is_ok());
            }
        }
    }

    #[derive(serde::Serialize)]
    struct PinFrame { pins: Vec<bool> }

    // ── Command socket ─────────────────────────────────────────────────────

    pub fn start_cmd_server(running: Arc<AtomicBool>, cmd_tx: mpsc::Sender<CmdRequest>) {
        let _ = std::fs::remove_file(super::CMD_SOCKET_PATH);

        thread::Builder::new()
            .name("gpionext-cmd".into())
            .spawn(move || cmd_server_loop(running, cmd_tx))
            .expect("spawn ipc command thread");
    }

    fn cmd_server_loop(running: Arc<AtomicBool>, cmd_tx: mpsc::Sender<CmdRequest>) {
        let listener = match UnixListener::bind(super::CMD_SOCKET_PATH) {
            Ok(l) => l,
            Err(e) => { eprintln!("[gpionext-cmd] bind failed: {e}"); return; }
        };
        listener.set_nonblocking(true).expect("set_nonblocking on cmd listener");

        while running.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let tx = cmd_tx.clone();
                    thread::Builder::new()
                        .name("gpionext-cmd-conn".into())
                        .spawn(move || handle_cmd_connection(stream, tx))
                        .ok();
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    eprintln!("[gpionext-cmd] accept error: {e}");
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }

        let _ = std::fs::remove_file(super::CMD_SOCKET_PATH);
    }

    fn handle_cmd_connection(
        stream: std::os::unix::net::UnixStream,
        tx: mpsc::Sender<CmdRequest>,
    ) {
        let mut write_stream = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };
        let reader = BufReader::new(stream);

        for line in reader.lines() {
            let line = match line {
                Ok(l) if !l.trim().is_empty() => l,
                _ => break,
            };

            let cmd = match parse_cmd(&line) {
                Some(c) => c,
                None => {
                    let _ = write_stream.write_all(b"{\"ok\":false,\"msg\":\"unknown command\"}\n");
                    break;
                }
            };

            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            if tx.send((cmd, reply_tx)).is_err() {
                let _ = write_stream
                    .write_all(b"{\"ok\":false,\"msg\":\"daemon not available\"}\n");
                break;
            }

            let response = reply_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| r#"{"ok":false,"msg":"daemon timeout"}"#.into());

            let _ = write_stream.write_all(response.as_bytes());
            let _ = write_stream.write_all(b"\n");
            break; // one command per connection
        }
    }

    fn parse_cmd(line: &str) -> Option<DaemonCmd> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let cmd = v.get("cmd")?.as_str()?;

        match cmd {
            "reload" => Some(DaemonCmd::Reload),
            "stop"   => Some(DaemonCmd::Stop),
            "status" => Some(DaemonCmd::Status),
            "set_combo_delay" => {
                let ms = v.get("value")?.as_u64()? as u32;
                Some(DaemonCmd::SetComboDelay(ms))
            }
            "set_debounce" => {
                let ms = v.get("value")?.as_u64()? as u32;
                Some(DaemonCmd::SetDebounce(ms))
            }
            "set_pins" => {
                let s = v.get("value")?.as_str()?;
                let pins: Vec<u8> = s.split(',')
                    .filter_map(|t| t.trim().parse::<u8>().ok())
                    .collect();
                if pins.is_empty() { return None; }
                Some(DaemonCmd::SetPins(pins))
            }
            "use_i2c" => {
                let enabled = v.get("value")?.as_bool()?;
                Some(DaemonCmd::UseI2c(enabled))
            }
            _ => None,
        }
    }
}

// Re-export the public API so callers use `ipc::start_ipc_server` etc.
#[cfg(unix)]
pub use unix_impl::{start_ipc_server, start_cmd_server};
