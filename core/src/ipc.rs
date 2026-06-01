/// ipc.rs — Unix socket IPC server for the GPIOnext daemon.
///
/// Broadcasts pin state to connected clients (e.g. gpionext-config live pin
/// view) every 50ms over a Unix domain socket as newline-delimited JSON:
///
///   {"pins":[false,true,false,...]}
///
/// Only active on Linux. Clients connect to `/tmp/gpionext.sock` and read
/// frames until EOF (daemon stopped) or disconnect.

use std::{
    io::Write,
    os::unix::net::UnixListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

pub const SOCKET_PATH: &str = "/tmp/gpionext.sock";

/// Spawn the IPC broadcaster thread. Returns immediately.
///
/// The thread stops automatically when `running` is set to `false`.
pub fn start_ipc_server(running: Arc<AtomicBool>) {
    // Remove stale socket file from a previous run
    let _ = std::fs::remove_file(SOCKET_PATH);

    thread::Builder::new()
        .name("gpionext-ipc".into())
        .spawn(move || {
            ipc_server_loop(running);
        })
        .expect("spawn ipc server thread");
}

fn ipc_server_loop(running: Arc<AtomicBool>) {
    let listener = match UnixListener::bind(SOCKET_PATH) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[gpionext-ipc] bind failed: {e}");
            return;
        }
    };
    // Non-blocking accept so we can check `running`
    listener
        .set_nonblocking(true)
        .expect("set_nonblocking on UnixListener");

    // Shared list of connected client writers
    let clients: Arc<Mutex<Vec<std::os::unix::net::UnixStream>>> =
        Arc::new(Mutex::new(Vec::new()));

    // Broadcaster thread: wake every 50ms, push to all clients
    {
        let clients2 = Arc::clone(&clients);
        let running2 = Arc::clone(&running);
        thread::Builder::new()
            .name("gpionext-ipc-bcast".into())
            .spawn(move || {
                broadcast_loop(clients2, running2);
            })
            .expect("spawn ipc broadcast thread");
    }

    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Ok(mut c) = clients.lock() {
                    c.push(stream);
                }
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

    let _ = std::fs::remove_file(SOCKET_PATH);
}

fn broadcast_loop(
    clients: Arc<Mutex<Vec<std::os::unix::net::UnixStream>>>,
    running: Arc<AtomicBool>,
) {
    while running.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));

        // Read current 256-bit bitmask from the global atomic state
        let words = crate::bitmask::current_bitmask();
        let pins: Vec<bool> = (0..256usize)
            .map(|i| {
                let word = i / 64;
                let bit = i % 64;
                (words[word] >> bit) & 1 == 1
            })
            .collect();

        let frame = match serde_json::to_string(&PinFrame { pins }) {
            Ok(s) => format!("{s}\n"),
            Err(_) => continue,
        };
        let bytes = frame.as_bytes();

        if let Ok(mut guard) = clients.lock() {
            guard.retain_mut(|stream| {
                stream.write_all(bytes).is_ok()
            });
        }
    }
}

#[derive(serde::Serialize)]
struct PinFrame {
    pins: Vec<bool>,
}
