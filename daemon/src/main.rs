/// gpionext — native Rust daemon.
///
/// Replaces `python/gpionext.py` for platforms without a Python runtime
/// (Recalbox, Batocera). Reads the same `gpionext.json` that `gpionext-config`
/// writes, drives GPIO → uinput via the `core` crate, and exposes two Unix
/// sockets:
///   /tmp/gpionext.sock     — 50 ms pin-state broadcast (read by live pin view)
///   /tmp/gpionext-cmd.sock — request/response command channel

mod config;
mod log;

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

use gpionext_core::{bitmask, gpio, ipc, uinput};

#[cfg(feature = "i2c")]
use gpionext_core::i2c;

use config::{effective_pins, parse_hex_addr, parse_int_pin, GpioConfig};
use log::Logger;

// ---------------------------------------------------------------------------
// CLI argument parsing (no external dep — hand-rolled)
// ---------------------------------------------------------------------------

struct Args {
    config_path: PathBuf,
    combo_delay: Option<u32>,
    key_hold_delay: Option<u32>,
    debounce: Option<u32>,
    pins: Option<String>,
    pulldown: bool,
    use_i2c: bool,
    dev: bool,
    debug: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        config_path: PathBuf::from("/opt/gpionext/config/gpionext.json"),
        combo_delay: None,
        key_hold_delay: None,
        debounce: None,
        pins: None,
        pulldown: false,
        use_i2c: false,
        dev: false,
        debug: false,
    };

    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--config" => {
                i += 1;
                if i < raw.len() {
                    args.config_path = PathBuf::from(&raw[i]);
                }
            }
            "--combo_delay"    => { i += 1; if i < raw.len() { args.combo_delay    = raw[i].parse().ok(); } }
            "--key_hold_delay" => { i += 1; if i < raw.len() { args.key_hold_delay = raw[i].parse().ok(); } }
            "--debounce"       => { i += 1; if i < raw.len() { args.debounce       = raw[i].parse().ok(); } }
            "--pins"           => { i += 1; if i < raw.len() { args.pins           = Some(raw[i].clone()); } }
            "--pulldown" => args.pulldown = true,
            "--use_i2c"  => args.use_i2c = true,
            "--dev"      => args.dev = true,
            "--debug"    => args.debug = true,
            other => eprintln!("[gpionext] unknown argument: {other}"),
        }
        i += 1;
    }

    args
}

// ---------------------------------------------------------------------------
// Runtime settings — merged from JSON + CLI overrides
// ---------------------------------------------------------------------------

struct RuntimeSettings {
    combo_delay: u32,
    key_hold_delay: u32,
    debounce: u32,
    pins: Vec<u8>,
    pulldown: bool,
    use_i2c: bool,
}

impl RuntimeSettings {
    fn from(cfg: &GpioConfig, args: &Args) -> Self {
        let d = &cfg.daemon;
        let pins = if let Some(ref p) = args.pins {
            let mut fake = d.clone();
            fake.pins = p.clone();
            effective_pins(&fake)
        } else {
            effective_pins(d)
        };
        Self {
            combo_delay:    args.combo_delay.unwrap_or(d.combo_delay),
            key_hold_delay: args.key_hold_delay.unwrap_or(d.key_hold_delay),
            debounce:       args.debounce.unwrap_or(d.debounce),
            pins,
            pulldown:  args.pulldown || d.pulldown,
            // Enable I2C if explicitly requested on CLI, or if the config has chips configured.
        use_i2c: args.use_i2c
            || !cfg.i2c.mcp23017.is_empty()
            || !cfg.i2c.ads1115.is_empty()
            || !cfg.i2c.pcf8574.is_empty(),
        }
    }
}

// ---------------------------------------------------------------------------
// DaemonCore — owns the running GPIO/uinput/I2C threads
// ---------------------------------------------------------------------------

struct DaemonCore {
    gpio_loop: Option<gpio::GpioLoop>,
    i2c_threads: Vec<std::thread::JoinHandle<()>>,
    /// Dedicated stop flag for I2C poll threads (separate from the main running flag
    /// so that I2C can be stopped independently on SetPins / UseI2c commands).
    i2c_running: Arc<AtomicBool>,
}

impl Default for DaemonCore {
    /// Returns a hollow core (no loops running). Used as a placeholder during
    /// stop-then-restart sequences so we can `take` the old core out cleanly.
    fn default() -> Self {
        Self {
            gpio_loop: None,
            i2c_threads: Vec::new(),
            i2c_running: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl DaemonCore {
    fn start(cfg: &GpioConfig, rt: &RuntimeSettings, logger: &Logger) -> Self {
        let peripherals = config::build_peripherals(cfg);
        logger.log(&format!("Starting with {} peripherals", peripherals.len()));

        bitmask::init_pool(8);
        let config_arc = Arc::new(bitmask::build_config(
            peripherals,
            rt.combo_delay as u64,
            rt.key_hold_delay as u64,
        ));
        bitmask::set_config_arc(config_arc.clone());
        uinput::open_all(&config_arc);

        // Warn about I2C pins in the active list
        for &p in &rt.pins {
            if p == 3 || p == 5 {
                logger.log(&format!(
                    "WARNING: BOARD pin {p} is an I2C pin (SDA/SCL). \
                     Remove it from the pin list or use --use_i2c."
                ));
            }
        }

        let gpio_cfg = gpio::GpioConfig {
            pins: rt.pins.clone(),
            pulldown: rt.pulldown,
            debounce_ms: rt.debounce,
        };
        let gpio_loop = match gpio::GpioLoop::run(&gpio_cfg, &[]) {
            Ok(lp) => { logger.log("GPIO event loop started"); Some(lp) }
            Err(e) => { eprintln!("[gpionext] GPIO start failed: {e}"); None }
        };

        let i2c_running = Arc::new(AtomicBool::new(true));
        let i2c_threads = if rt.use_i2c {
            start_i2c(cfg, Arc::clone(&i2c_running), logger)
        } else {
            Vec::new()
        };

        DaemonCore { gpio_loop, i2c_threads, i2c_running }
    }

    /// Stop all threads and release uinput devices. Consumes self.
    fn stop(mut self, logger: &Logger) {
        self.i2c_running.store(false, Ordering::SeqCst);
        if let Some(lp) = self.gpio_loop.take() {
            lp.stop();
        }
        for t in self.i2c_threads.drain(..) {
            let _ = t.join();
        }
        uinput::close_all();
        logger.log("Core stopped");
    }
}

/// Stop `old` core and start a fresh one. Ensures old threads are joined before
/// new GPIO lines are opened (prevents "line already in use" errors).
fn restart_core(
    core: &mut DaemonCore,
    cfg: &GpioConfig,
    rt: &RuntimeSettings,
    logger: &Logger,
) {
    let old = std::mem::replace(core, DaemonCore::default());
    old.stop(logger);
    *core = DaemonCore::start(cfg, rt, logger);
}

// ---------------------------------------------------------------------------
// I2C driver startup
// ---------------------------------------------------------------------------

fn start_i2c(
    cfg: &GpioConfig,
    #[allow(unused_variables)] running: Arc<AtomicBool>,
    logger: &Logger,
) -> Vec<std::thread::JoinHandle<()>> {
    let mut threads = Vec::new();

    #[cfg(feature = "i2c")]
    {
        for entry in &cfg.i2c.mcp23017 {
            if let Some(addr) = parse_hex_addr(&entry.address) {
                let int_pin = parse_int_pin(&entry.int_pin);
                match i2c::Mcp23017::new(entry.bus, addr, int_pin) {
                    Ok(dev) => {
                        let r = Arc::clone(&running);
                        threads.push(std::thread::spawn(move || dev.poll(r)));
                        logger.log(&format!("MCP23017 @ {} bus {} started", entry.address, entry.bus));
                    }
                    Err(e) => eprintln!("[gpionext] MCP23017 init failed: {e:?}"),
                }
            }
        }
        for entry in &cfg.i2c.pcf8574 {
            if let Some(addr) = parse_hex_addr(&entry.address) {
                let int_pin = parse_int_pin(&entry.int_pin);
                match i2c::Pcf8574::new(entry.bus, addr, int_pin) {
                    Ok(dev) => {
                        let r = Arc::clone(&running);
                        threads.push(std::thread::spawn(move || dev.poll(r)));
                        logger.log(&format!("PCF8574 @ {} bus {} started", entry.address, entry.bus));
                    }
                    Err(e) => eprintln!("[gpionext] PCF8574 init failed: {e:?}"),
                }
            }
        }
        for entry in &cfg.i2c.ads1115 {
            if let Some(addr) = parse_hex_addr(&entry.address) {
                match i2c::Ads1115::new(entry.bus, addr) {
                    Ok(dev) => {
                        let r = Arc::clone(&running);
                        threads.push(std::thread::spawn(move || dev.poll(r)));
                        logger.log(&format!("ADS1115 @ {} bus {} started", entry.address, entry.bus));
                    }
                    Err(e) => eprintln!("[gpionext] ADS1115 init failed: {e:?}"),
                }
            }
        }
    }

    #[cfg(not(feature = "i2c"))]
    {
        let _ = logger;
        if !cfg.i2c.mcp23017.is_empty()
            || !cfg.i2c.ads1115.is_empty()
            || !cfg.i2c.pcf8574.is_empty()
        {
            eprintln!(
                "[gpionext] WARNING: I2C chips configured but daemon built without i2c feature"
            );
        }
    }

    threads
}

// ---------------------------------------------------------------------------
// Signal handling (Unix only)
// ---------------------------------------------------------------------------

// Process-global pointers to AtomicBools, set once before any signal can fire.
// Defined unconditionally so setup_signals compiles on all platforms.
static STOP_PTR:   AtomicUsize = AtomicUsize::new(0);
static RELOAD_PTR: AtomicUsize = AtomicUsize::new(0);

#[cfg(unix)]
extern "C" fn signal_handler_stop(_: libc::c_int) {
    let ptr = STOP_PTR.load(Ordering::Relaxed);
    if ptr != 0 {
        // SAFETY: pointer is valid for the daemon lifetime; AtomicBool is lock-free
        // and safe to write from a signal handler.
        unsafe { &*(ptr as *const AtomicBool) }.store(false, Ordering::SeqCst);
    }
}

#[cfg(unix)]
extern "C" fn signal_handler_reload(_: libc::c_int) {
    let ptr = RELOAD_PTR.load(Ordering::Relaxed);
    if ptr != 0 {
        unsafe { &*(ptr as *const AtomicBool) }.store(true, Ordering::SeqCst);
    }
}

fn setup_signals(running: &Arc<AtomicBool>, reload: &Arc<AtomicBool>) {
    STOP_PTR.store(Arc::as_ptr(running) as usize, Ordering::SeqCst);
    RELOAD_PTR.store(Arc::as_ptr(reload) as usize, Ordering::SeqCst);

    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGTERM, signal_handler_stop   as libc::sighandler_t);
        libc::signal(libc::SIGINT,  signal_handler_stop   as libc::sighandler_t);
        libc::signal(libc::SIGQUIT, signal_handler_stop   as libc::sighandler_t);
        libc::signal(libc::SIGHUP,  signal_handler_reload as libc::sighandler_t);
    }
}

// ---------------------------------------------------------------------------
// PID file helpers
// ---------------------------------------------------------------------------

fn write_pid_file() {
    if let Err(e) = std::fs::write("/run/gpionext.pid", format!("{}\n", std::process::id())) {
        eprintln!("[gpionext] WARNING: could not write PID file: {e}");
    }
}

fn remove_pid_file() {
    let _ = std::fs::remove_file("/run/gpionext.pid");
}

// ---------------------------------------------------------------------------
// Status response
// ---------------------------------------------------------------------------

fn status_response(start_time: Instant) -> String {
    let uptime = start_time.elapsed().as_secs();
    format!(
        r#"{{"ok":true,"version":"{}","uptime_s":{uptime}}}"#,
        gpionext_core::VERSION
    )
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let args = parse_args();
    let logger = Logger::new(args.dev, args.debug);

    logger.log(&format!(
        "GPIOnext {} starting (config: {})",
        gpionext_core::VERSION,
        args.config_path.display()
    ));

    let mut cfg = match config::load(&args.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[gpionext] Failed to load config: {e}");
            GpioConfig::default()
        }
    };
    let mut rt = RuntimeSettings::from(&cfg, &args);

    // Signal flags
    let running     = Arc::new(AtomicBool::new(true));
    let reload_flag = Arc::new(AtomicBool::new(false));
    setup_signals(&running, &reload_flag);

    // IPC sockets
    #[cfg(unix)]
    ipc::start_ipc_server(Arc::clone(&running));

    let (cmd_tx, cmd_rx) = mpsc::channel::<ipc::CmdRequest>();
    #[cfg(unix)]
    ipc::start_cmd_server(Arc::clone(&running), cmd_tx);
    #[cfg(not(unix))]
    let _ = cmd_tx; // not used on non-Unix

    write_pid_file();

    // Start GPIO / uinput / I2C
    let mut core = DaemonCore::start(&cfg, &rt, &logger);
    let start_time = Instant::now();

    logger.log("Running — waiting for GPIO events");

    // ── Main loop ─────────────────────────────────────────────────────────────
    loop {
        std::thread::sleep(Duration::from_millis(100));

        if !running.load(Ordering::Relaxed) {
            logger.log("Shutdown signal received");
            break;
        }

        // SIGHUP reload
        if reload_flag.swap(false, Ordering::Relaxed) {
            logger.log("SIGHUP: reloading config");
            match config::load(&args.config_path) {
                Ok(c) => { cfg = c; }
                Err(e) => { eprintln!("[gpionext] reload failed: {e}"); continue; }
            }
            rt = RuntimeSettings::from(&cfg, &args);
            restart_core(&mut core, &cfg, &rt, &logger);
            logger.log("Config reloaded");
        }

        // IPC commands
        while let Ok((cmd, reply)) = cmd_rx.try_recv() {
            use ipc::DaemonCmd;
            let response: String = match cmd {
                DaemonCmd::Reload => {
                    match config::load(&args.config_path) {
                        Err(e) => format!(r#"{{"ok":false,"msg":"load error: {e}"}}"#),
                        Ok(c)  => {
                            cfg = c;
                            rt = RuntimeSettings::from(&cfg, &args);
                            restart_core(&mut core, &cfg, &rt, &logger);
                            logger.log("Reloaded via IPC");
                            r#"{"ok":true,"msg":"Config reloaded"}"#.into()
                        }
                    }
                }
                DaemonCmd::Stop => {
                    running.store(false, Ordering::SeqCst);
                    r#"{"ok":true,"msg":"Stopping"}"#.into()
                }
                DaemonCmd::Status => status_response(start_time),
                DaemonCmd::SetComboDelay(ms) => {
                    rt.combo_delay = ms;
                    cfg.daemon.combo_delay = ms;
                    let _ = config::save(&args.config_path, &cfg);
                    restart_core(&mut core, &cfg, &rt, &logger);
                    logger.log(&format!("combo_delay set to {ms}ms"));
                    format!(r#"{{"ok":true,"msg":"combo_delay={ms}ms"}}"#)
                }
                DaemonCmd::SetDebounce(ms) => {
                    rt.debounce = ms;
                    cfg.daemon.debounce = ms;
                    let _ = config::save(&args.config_path, &cfg);
                    restart_core(&mut core, &cfg, &rt, &logger);
                    logger.log(&format!("debounce set to {ms}ms"));
                    format!(r#"{{"ok":true,"msg":"debounce={ms}ms"}}"#)
                }
                DaemonCmd::SetPins(pins) => {
                    let pin_str = pins.iter().map(|p| p.to_string())
                        .collect::<Vec<_>>().join(",");
                    rt.pins = pins;
                    cfg.daemon.pins = pin_str.clone();
                    let _ = config::save(&args.config_path, &cfg);
                    restart_core(&mut core, &cfg, &rt, &logger);
                    logger.log(&format!("pins set to [{pin_str}]"));
                    format!(r#"{{"ok":true,"msg":"pins=[{pin_str}]"}}"#)
                }
                DaemonCmd::UseI2c(enabled) => {
                    rt.use_i2c = enabled;
                    restart_core(&mut core, &cfg, &rt, &logger);
                    logger.log(&format!("use_i2c={enabled}"));
                    format!(r#"{{"ok":true,"msg":"use_i2c={enabled}"}}"#)
                }
            };
            let _ = reply.send(response);
        }
    }

    // ── Graceful shutdown ─────────────────────────────────────────────────────
    logger.log("Shutting down");
    core.stop(&logger);
    remove_pid_file();
    logger.log("Done");
}
