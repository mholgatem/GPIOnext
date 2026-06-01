/// init_sys.rs — Init system detection and daemon lifecycle control.
///
/// Detects systemd / S6 (Batocera) / OpenRC (Recalbox) / PID-file fallback
/// at runtime and dispatches start/stop/reload commands to the right tool.

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

/// Execute a daemon lifecycle command using the appropriate init system.
pub fn run_daemon_cmd(cmd: DaemonCmd) -> Result<()> {
    match detect_init() {
        InitSystem::Systemd => systemd_cmd(cmd),
        InitSystem::S6 => s6_cmd(cmd),
        InitSystem::OpenRC => openrc_cmd(cmd),
        InitSystem::PidFile => pidflie_cmd(cmd),
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
    // Try s6-svc first, fall back to s6-rc
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
    // Try /etc/init.d script first, then service command
    let init_script = "/etc/init.d/gpionext";
    if Path::new(init_script).exists() {
        run(&[init_script, arg])
    } else {
        run(&["rc-service", "gpionext", arg])
    }
}

fn pidflie_cmd(cmd: DaemonCmd) -> Result<()> {
    let pid_path = "/run/gpionext.pid";
    match cmd {
        DaemonCmd::Start => {
            run(&["/usr/bin/gpionext", "start"])
        }
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
fn libc_sighup() -> i32  { libc::SIGHUP  }
#[cfg(not(unix))]
fn libc_sigterm() -> i32 { 15 }
#[cfg(not(unix))]
fn libc_sighup() -> i32  { 1 }

fn run(argv: &[&str]) -> Result<()> {
    let status = Command::new(argv[0]).args(&argv[1..]).status()?;
    if !status.success() {
        bail!("{} exited with {status}", argv.join(" "));
    }
    Ok(())
}
