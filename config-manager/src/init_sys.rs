/// init_sys.rs — Init system detection and daemon lifecycle control.
///
/// For Stop and Reload, tries the Rust daemon's IPC command socket first
/// (`/tmp/gpionext-cmd.sock`). If the socket is unavailable (daemon not running
/// or using the old Python daemon), falls back to the detected init system.
///
/// Start always goes through the init system (can't start via an absent socket).

use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InitSystem {
    Systemd,
    S6,
    OpenRC,
    PidFile,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DaemonCmd {
    Start,
    Stop,
    Reload,
}

pub fn detect_init() -> InitSystem {
    if Path::new("/run/systemd/private").exists() {
        InitSystem::Systemd
    } else if Path::new("/run/s6").exists() || Path::new("/run/s6-rc").exists() {
        InitSystem::S6
    } else if Path::new("/sbin/openrc").exists() || Path::new("/usr/sbin/openrc").exists() {
        InitSystem::OpenRC
    } else {
        InitSystem::PidFile
    }
}

/// Execute a daemon lifecycle command.
///
/// For Stop/Reload: tries the IPC command socket first (works with the native
/// Rust daemon), then falls back to the detected init system.
/// Start always goes through the init system.
pub fn run_daemon_cmd(cmd: DaemonCmd) -> Result<()> {
    match cmd {
        DaemonCmd::Start => dispatch_init_cmd(cmd),
        DaemonCmd::Stop | DaemonCmd::Reload => {
            if try_socket_cmd(cmd).is_ok() {
                return Ok(());
            }
            dispatch_init_cmd(cmd)
        }
    }
}

// ---------------------------------------------------------------------------
// IPC command socket (Rust daemon only)
// ---------------------------------------------------------------------------

/// Send a command to the running Rust daemon via the command socket.
/// Returns `Ok(())` if the daemon acknowledged; `Err` otherwise.
#[cfg(unix)]
fn try_socket_cmd(cmd: DaemonCmd) -> Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let mut stream = UnixStream::connect("/tmp/gpionext-cmd.sock")?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    let req = match cmd {
        DaemonCmd::Stop   => r#"{"cmd":"stop"}"#,
        DaemonCmd::Reload => r#"{"cmd":"reload"}"#,
        DaemonCmd::Start  => return Err(anyhow::anyhow!("start not supported via socket")),
    };

    stream.write_all(req.as_bytes())?;
    stream.write_all(b"\n")?;

    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;

    let v: serde_json::Value = serde_json::from_str(line.trim())
        .unwrap_or(serde_json::Value::Null);
    if v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false) {
        Ok(())
    } else {
        let msg = v.get("msg").and_then(|m| m.as_str()).unwrap_or("unknown error");
        bail!("daemon returned error: {msg}")
    }
}

#[cfg(not(unix))]
fn try_socket_cmd(_cmd: DaemonCmd) -> Result<()> {
    bail!("IPC socket not supported on this platform")
}

// ---------------------------------------------------------------------------
// Init system dispatch
// ---------------------------------------------------------------------------

fn dispatch_init_cmd(cmd: DaemonCmd) -> Result<()> {
    match detect_init() {
        InitSystem::Systemd => systemd_cmd(cmd),
        InitSystem::S6 => s6_cmd(cmd),
        InitSystem::OpenRC => openrc_cmd(cmd),
        InitSystem::PidFile => pidfile_cmd(cmd),
    }
}

fn systemd_cmd(cmd: DaemonCmd) -> Result<()> {
    let arg = match cmd {
        DaemonCmd::Start => "start",
        DaemonCmd::Stop => "stop",
        DaemonCmd::Reload => "reload",
    };
    run(&["systemctl", arg, "gpionext"])
}

fn s6_cmd(cmd: DaemonCmd) -> Result<()> {
    let arg = match cmd {
        DaemonCmd::Start => "-u",
        DaemonCmd::Stop => "-d",
        DaemonCmd::Reload => "-h",
    };
    let svc_dir = "/run/service/gpionext";
    if Path::new(svc_dir).exists() {
        run(&["s6-svc", arg, svc_dir])
    } else {
        let rc_arg = match cmd {
            DaemonCmd::Start => "up",
            DaemonCmd::Stop => "down",
            DaemonCmd::Reload => "reload",
        };
        run(&["s6-rc", "-u", rc_arg, "gpionext"])
    }
}

fn openrc_cmd(cmd: DaemonCmd) -> Result<()> {
    let arg = match cmd {
        DaemonCmd::Start => "start",
        DaemonCmd::Stop => "stop",
        DaemonCmd::Reload => "reload",
    };
    let init_script = "/etc/init.d/gpionext";
    if Path::new(init_script).exists() {
        run(&[init_script, arg])
    } else {
        run(&["rc-service", "gpionext", arg])
    }
}

fn pidfile_cmd(cmd: DaemonCmd) -> Result<()> {
    let pid_path = "/run/gpionext.pid";
    match cmd {
        DaemonCmd::Start => run(&["/usr/bin/gpionext", "start"]),
        DaemonCmd::Stop => {
            let pid = read_pid(pid_path)?;
            send_signal(pid, libc_sigterm())
        }
        DaemonCmd::Reload => {
            let pid = read_pid(pid_path)?;
            send_signal(pid, libc_sighup())
        }
    }
}

fn read_pid(path: &str) -> Result<u32> {
    let s = std::fs::read_to_string(path)?;
    Ok(s.trim().parse()?)
}

fn send_signal(pid: u32, sig: i32) -> Result<()> {
    #[cfg(unix)]
    unsafe {
        if libc::kill(pid as libc::pid_t, sig) != 0 {
            bail!(
                "kill({pid}, {sig}) failed: {}",
                std::io::Error::last_os_error()
            );
        }
        return Ok(());
    }
    #[cfg(not(unix))]
    bail!("signal delivery not supported on this platform");
}

#[cfg(unix)]
fn libc_sigterm() -> i32 { libc::SIGTERM }
#[cfg(unix)]
fn libc_sighup()  -> i32 { libc::SIGHUP }
#[cfg(not(unix))]
fn libc_sigterm() -> i32 { 15 }
#[cfg(not(unix))]
fn libc_sighup()  -> i32 { 1 }

fn run(argv: &[&str]) -> Result<()> {
    let status = Command::new(argv[0]).args(&argv[1..]).status()?;
    if !status.success() {
        bail!("{} exited with {status}", argv.join(" "));
    }
    Ok(())
}
