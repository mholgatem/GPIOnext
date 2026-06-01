use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
/// gpionext_core — PyO3 extension module
///
/// Exposes the following to Python:
///   - `GpioCore`         : lifecycle manager (start / stop / reload)
///   - `get_pin_states()` : returns current pressed-pin bitmask (for live UI)
///   - `version()`        : returns the crate semver string
///
/// Python usage:
/// ```python
/// import gpionext_core
/// core = gpionext_core.GpioCore()
/// core.start(config_dict)   # config_dict loaded from SQLite by SQL.py
/// # ... daemon sleeps, GPIO events are handled in Rust threads ...
/// core.reload(new_config_dict)  # called by SIGHUP handler
/// core.stop()
/// ```
use std::sync::Arc;

mod bitmask;
mod gpio;
mod i2c;
mod ipc;
mod uinput;

use bitmask::{build_config, init_pool, set_config, set_config_arc, EventType, Peripheral};

// ---------------------------------------------------------------------------
// Module-level functions
// ---------------------------------------------------------------------------

/// Returns the semver version string of this compiled extension.
///
/// # Returns
/// `str` — e.g. `"0.1.0"`
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Returns true when this extension was compiled with I2C driver support.
///
/// The Python UI uses this to warn when I2C chips are configured but the
/// installed Rust extension was built without the `i2c` feature; in that case
/// virtual I2C pins can be displayed from SQLite but will never change state.
#[pyfunction]
fn i2c_enabled() -> bool {
    cfg!(feature = "i2c")
}

/// Returns the current pressed-pin bitmask as a single arbitrary-precision integer.
/// This allows Python code to perform bitwise operations directly (e.g. `bitmask & (1 << pin)`).
/// Now supports up to 256 bits (64 physical GPIO + 192 virtual I2C).
///
/// # Returns
/// `int` — current bitmask of all 256 potential pins
#[pyfunction]
fn get_pin_states(py: Python<'_>) -> PyResult<PyObject> {
    let words = bitmask::current_bitmask();
    let mut bytes = [0u8; 32];
    bytes[0..8].copy_from_slice(&words[0].to_le_bytes());
    bytes[8..16].copy_from_slice(&words[1].to_le_bytes());
    bytes[16..24].copy_from_slice(&words[2].to_le_bytes());
    bytes[24..32].copy_from_slice(&words[3].to_le_bytes());

    // Construct a Python 'int' from these 24 bytes (little-endian, unsigned)
    let int_obj = py
        .get_type::<pyo3::types::PyLong>()
        .call_method1("from_bytes", (bytes.as_slice(), "little"))?;

    Ok(int_obj.unbind())
}

// ---------------------------------------------------------------------------
// GpioCore — lifecycle manager
// ---------------------------------------------------------------------------

/// Lifecycle manager for the GPIOnext hot path.
///
/// Owns the Rayon thread pool, the GPIO event loop thread,
/// and the active configuration. All fields are managed internally;
/// Python only calls `start`, `stop`, and `reload`.
#[pyclass]
struct GpioCore {
    gpio_loop: Option<gpio::GpioLoop>,
    i2c_threads: Vec<std::thread::JoinHandle<()>>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

#[pymethods]
impl GpioCore {
    #[new]
    fn new() -> Self {
        GpioCore {
            gpio_loop: None,
            i2c_threads: Vec::new(),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the GPIO event loop and initialise the Rayon thread pool.
    ///
    /// # Parameters
    /// - `config`: dict with keys:
    ///     - `peripherals` (list[dict]) — one dict per button/key/axis/command:
    ///         - `name` (str)
    ///         - `device_index` (int, 0-5)
    ///         - `pins` (list[int])  — BOARD pin numbers
    ///         - `type` (str)        — "BUTTON" | "KEY" | "AXIS" | "COMMAND"
    ///         - `command` (str|int) — evdev code or bash string
    ///     - `combo_delay` (int, ms)
    ///     - `key_hold_delay` (int, ms, default 350)
    ///     - `pins` (list[int])     — all BOARD pins to watch
    ///     - `pulldown` (bool)
    ///     - `debounce` (int, ms)
    ///     - `skip_pins` (list[int]) — pins reserved by audio HAT detection
    ///
    /// # Errors
    /// Raises `RuntimeError` if GPIO setup fails (missing module, bad pin, etc.)
    fn start(&mut self, config: &Bound<'_, PyDict>) -> PyResult<()> {
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let combo_delay: u64 = config
            .get_item("combo_delay")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(50);
        let key_hold_delay: u64 = config
            .get_item("key_hold_delay")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(350);
        let pulldown: bool = config
            .get_item("pulldown")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(false);
        let debounce: u32 = config
            .get_item("debounce")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(1);

        let pins: Vec<u8> = config
            .get_item("pins")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();
        let skip_pins: Vec<u8> = config
            .get_item("skip_pins")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();

        // Parse peripheral list
        let peripherals = parse_peripherals(config)?;

        // Initialise fixed thread pool (8 workers; never grows)
        init_pool(8);

        // Install config into global state
        let config_obj = build_config(peripherals, combo_delay, key_hold_delay);
        let config_arc = Arc::new(config_obj);
        set_config_arc(config_arc.clone());

        // Initialise uinput devices
        uinput::open_all(&config_arc);

        // Start GPIO event loop (stub until libgpiod feature enabled)
        let gpio_config = gpio::GpioConfig {
            pins,
            pulldown,
            debounce_ms: debounce,
        };
        match gpio::GpioLoop::run(&gpio_config, &skip_pins) {
            Ok(lp) => {
                self.gpio_loop = Some(lp);
            }
            Err(e) => return Err(pyo3::exceptions::PyRuntimeError::new_err(e.to_string())),
        }

        // Start I2C drivers if enabled
        #[cfg(feature = "i2c")]
        {
            if let Some(mcp_list) = config.get_item("i2c_mcp23017")? {
                let list = mcp_list.downcast::<PyList>()?;
                for item in list.iter() {
                    let d = item.downcast::<PyDict>()?;
                    let bus: u8 = d
                        .get_item("bus")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(1);
                    let addr: u8 = d
                        .get_item("address")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(0x20);
                    let int_pin = optional_u8(d, "int_pin")?;

                    if let Ok(mcp) = i2c::Mcp23017::new(bus, addr, int_pin) {
                        let r = self.running.clone();
                        self.i2c_threads
                            .push(std::thread::spawn(move || mcp.poll(r)));
                    }
                }
            }
            if let Some(pcf_list) = config.get_item("i2c_pcf8574")? {
                let list = pcf_list.downcast::<PyList>()?;
                for item in list.iter() {
                    let d = item.downcast::<PyDict>()?;
                    let bus: u8 = d
                        .get_item("bus")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(1);
                    let addr: u8 = d
                        .get_item("address")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(0x20);
                    let int_pin = optional_u8(d, "int_pin")?;

                    if let Ok(pcf) = i2c::Pcf8574::new(bus, addr, int_pin) {
                        let r = self.running.clone();
                        self.i2c_threads
                            .push(std::thread::spawn(move || pcf.poll(r)));
                    }
                }
            }
            if let Some(ads_list) = config.get_item("i2c_ads1115")? {
                let list = ads_list.downcast::<PyList>()?;
                for item in list.iter() {
                    let d = item.downcast::<PyDict>()?;
                    let bus: u8 = d
                        .get_item("bus")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(1);
                    let addr: u8 = d
                        .get_item("address")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(0x48);

                    if let Ok(ads) = i2c::Ads1115::new(bus, addr) {
                        let r = self.running.clone();
                        self.i2c_threads
                            .push(std::thread::spawn(move || ads.poll(r)));
                    }
                }
            }
        }

        // Start IPC server so gpionext-config can read live pin states
        #[cfg(unix)]
        ipc::start_ipc_server(Arc::clone(&self.running));

        Ok(())
    }

    /// Stop the GPIO event loop and flush all active uinput devices.
    fn stop(&mut self) -> PyResult<()> {
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if let Some(lp) = self.gpio_loop.take() {
            lp.stop();
        }
        for thread in self.i2c_threads.drain(..) {
            let _ = thread.join();
        }
        uinput::close_all();
        Ok(())
    }

    /// Hot-reload configuration on SIGHUP without restarting the daemon.
    /// Stops the current event loop, swaps config, restarts the loop.
    ///
    /// # Parameters
    /// - `config`: freshly loaded config dict (same schema as `start`)
    fn reload(&mut self, config: &Bound<'_, PyDict>) -> PyResult<()> {
        self.stop()?;
        self.start(config)
    }

    /// Start a lightweight GPIO monitor for the config tool.
    ///
    /// Same as `start()` but creates NO uinput devices — only the GPIO event
    /// loop and bitmask tracking are initialised. Used by config_manager.py
    /// so it can poll `get_pin_states()` while the user presses buttons,
    /// without needing a full daemon running.
    ///
    /// # Parameters
    /// - `config`: same schema as `start()`
    ///
    /// # Errors
    /// Raises `RuntimeError` if GPIO setup fails.
    fn start_monitor(&mut self, config: &Bound<'_, PyDict>) -> PyResult<()> {
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        init_pool(4); // smaller pool — config tool doesn't need combo resolution

        let pins: Vec<u8> = config
            .get_item("pins")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();
        let skip_pins: Vec<u8> = config
            .get_item("skip_pins")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();
        let pulldown: bool = config
            .get_item("pulldown")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(false);
        let debounce: u32 = config
            .get_item("debounce")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(1);

        // Install an empty config (no peripherals) so bitmask tracking works
        // but dispatch_press is never called
        set_config(build_config(vec![], 50, 350));

        let gpio_config = gpio::GpioConfig {
            pins,
            pulldown,
            debounce_ms: debounce,
        };
        match gpio::GpioLoop::run(&gpio_config, &skip_pins) {
            Ok(lp) => {
                self.gpio_loop = Some(lp);
            }
            Err(e) => return Err(pyo3::exceptions::PyRuntimeError::new_err(e.to_string())),
        }

        // Start I2C drivers if enabled
        #[cfg(feature = "i2c")]
        {
            if let Some(mcp_list) = config.get_item("i2c_mcp23017")? {
                let list = mcp_list.downcast::<PyList>()?;
                for item in list.iter() {
                    let d = item.downcast::<PyDict>()?;
                    let bus: u8 = d
                        .get_item("bus")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(1);
                    let addr: u8 = d
                        .get_item("address")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(0x20);
                    let int_pin = optional_u8(d, "int_pin")?;

                    if let Ok(mcp) = i2c::Mcp23017::new(bus, addr, int_pin) {
                        let r = self.running.clone();
                        self.i2c_threads
                            .push(std::thread::spawn(move || mcp.poll(r)));
                    }
                }
            }
            if let Some(pcf_list) = config.get_item("i2c_pcf8574")? {
                let list = pcf_list.downcast::<PyList>()?;
                for item in list.iter() {
                    let d = item.downcast::<PyDict>()?;
                    let bus: u8 = d
                        .get_item("bus")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(1);
                    let addr: u8 = d
                        .get_item("address")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(0x20);
                    let int_pin = optional_u8(d, "int_pin")?;

                    if let Ok(pcf) = i2c::Pcf8574::new(bus, addr, int_pin) {
                        let r = self.running.clone();
                        self.i2c_threads
                            .push(std::thread::spawn(move || pcf.poll(r)));
                    }
                }
            }
            if let Some(ads_list) = config.get_item("i2c_ads1115")? {
                let list = ads_list.downcast::<PyList>()?;
                for item in list.iter() {
                    let d = item.downcast::<PyDict>()?;
                    let bus: u8 = d
                        .get_item("bus")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(1);
                    let addr: u8 = d
                        .get_item("address")?
                        .map(|v| v.extract())
                        .transpose()?
                        .unwrap_or(0x48);

                    if let Ok(ads) = i2c::Ads1115::new(bus, addr) {
                        let r = self.running.clone();
                        self.i2c_threads
                            .push(std::thread::spawn(move || ads.poll(r)));
                    }
                }
            }
        }

        Ok(())
    }
}

fn optional_u8(d: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<u8>> {
    match d.get_item(key)? {
        Some(value) if !value.is_none() => value.extract().map(Some),
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Config parsing helpers
// ---------------------------------------------------------------------------

/// Parse the Python `peripherals` list from the config dict into typed Rust structs.
///
/// # Parameters
/// - `config`: the full config dict passed to `start()`
///
/// # Returns
/// `Vec<Peripheral>` — unsorted; `build_config` will sort them.
///
/// # Errors
/// Returns `PyValueError` if a peripheral dict is missing required keys or has
/// an unrecognised type string.
fn parse_peripherals(config: &Bound<'_, PyDict>) -> PyResult<Vec<Peripheral>> {
    use std::sync::atomic::{AtomicBool, AtomicU64};

    let raw_list = match config.get_item("peripherals")? {
        Some(v) => v,
        None => return Ok(Vec::new()),
    };
    let list = raw_list.downcast::<PyList>()?;

    let mut result = Vec::with_capacity(list.len());

    for item in list.iter() {
        let d = item.downcast::<PyDict>()?;

        let name: String = d
            .get_item("name")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();
        let device_index: usize = d
            .get_item("device_index")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(0);
        let type_str: String = d
            .get_item("type")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();
        let command: String = d
            .get_item("command")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();
        let pins: Vec<u8> = d
            .get_item("pins")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();

        // Build 256-bit pin bitmask from pin list
        let mut pin_mask: [u64; 4] = [0, 0, 0, 0];
        for &pin in &pins {
            let idx = (pin / 64) as usize;
            if idx < 4 {
                pin_mask[idx] |= 1u64 << (pin % 64);
            }
        }
        let pin_count = (pin_mask[0].count_ones()
            + pin_mask[1].count_ones()
            + pin_mask[2].count_ones()
            + pin_mask[3].count_ones()) as u8;

        // Parse event type
        let event_type = match type_str.as_str() {
            "BUTTON" => EventType::Button {
                evdev_code: command.parse::<u32>().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err(format!(
                        "BUTTON '{name}' command must be an integer evdev code, got '{command}'"
                    ))
                })?,
            },
            "KEY" => EventType::Key {
                evdev_code: command.parse::<u32>().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err(format!(
                        "KEY '{name}' command must be an integer evdev code, got '{command}'"
                    ))
                })?,
            },
            "AXIS" => {
                // command is "(evdev_type, evdev_code, press_value)" — same as reference
                let (et, ec, pv) = parse_axis_command(&command, &name)?;
                EventType::Axis {
                    evdev_type: et,
                    evdev_code: ec,
                    press_value: pv,
                }
            }
            "COMMAND" => EventType::Command {
                bash: command.clone(),
            },
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "Unknown peripheral type '{other}' for '{name}'"
                )))
            }
        };

        result.push(Peripheral {
            name,
            device_index: device_index.min(5),
            pin_mask,
            pin_count,
            event_type,
            is_pressed: AtomicBool::new(false),
            hold_generation: AtomicU64::new(0),
        });
    }

    Ok(result)
}

/// Parse an AXIS command string in the format `"(evdev_type, evdev_code, value)"`.
/// Matches the format used by the reference config/constants.py AXIS tuples.
///
/// # Parameters
/// - `s`    : the raw command string from the config DB
/// - `name` : peripheral name, used in error messages only
///
/// # Returns
/// `(evdev_type, evdev_code, press_value)`
fn parse_axis_command(s: &str, name: &str) -> PyResult<(u32, u32, i32)> {
    // Strip parentheses and split on comma
    let inner = s.trim().trim_start_matches('(').trim_end_matches(')');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "AXIS '{name}' command must be '(type, code, value)', got '{s}'"
        )));
    }
    let et = parts[0].trim().parse::<u32>().map_err(|_| {
        pyo3::exceptions::PyValueError::new_err(format!("AXIS '{name}' evdev_type not an int"))
    })?;
    let ec = parts[1].trim().parse::<u32>().map_err(|_| {
        pyo3::exceptions::PyValueError::new_err(format!("AXIS '{name}' evdev_code not an int"))
    })?;
    let pv = parts[2].trim().parse::<i32>().map_err(|_| {
        pyo3::exceptions::PyValueError::new_err(format!("AXIS '{name}' press_value not an int"))
    })?;
    Ok((et, ec, pv))
}

// ---------------------------------------------------------------------------
// PyO3 module registration
// ---------------------------------------------------------------------------

#[pymodule]
fn gpionext_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(i2c_enabled, m)?)?;
    m.add_function(wrap_pyfunction!(get_pin_states, m)?)?;
    m.add_class::<GpioCore>()?;
    Ok(())
}
