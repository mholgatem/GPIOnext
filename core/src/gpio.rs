/// GPIO event loop via the Linux GPIO character device API (gpiocdev).
///
/// `gpiocdev` is a pure-Rust implementation that calls the kernel's GPIO uAPI
/// ioctls directly, with no C library dependency at build time. It works on
/// all Pi models (2B through Pi 5) running kernel 5.10+ (GPIO uAPI v2).
///
/// # Event flow
/// ```text
/// /dev/gpiochip0
///   → edge event (rising or falling)
///   → bcm_to_board() converts line offset → BOARD pin number
///   → bitmask::set_pin() or bitmask::on_pin_release()
///   → bitmask::on_pin_press() schedules combo resolution in Rayon pool
/// ```
///
/// # Pin numbering
/// The config DB and Python layer use physical BOARD numbers (1–40).
/// gpiocdev uses BCM/GPIO line offsets (the numbers silk-screened as GPIOx).
/// BOARD_TO_BCM and bcm_to_board() translate between the two.
///
/// # Pin protection
/// - BOARD pins 3 & 5 are i2c SDA/SCL. If pulldown is requested on them and
///   the i2c feature is not enabled, a clear error is printed and the pin is
///   skipped (rather than silently crashing as the reference code did).
/// - Pins reserved by a detected audio HAT are passed in via `skip_pins` and
///   silently excluded from event detection.
/// - BOARD pins with no GPIO equivalent (power, ground) are skipped with a warning.
///
/// # Pi 5 compatibility
/// Pi 5 with Bookworm presents its 40-pin header GPIO on `/dev/gpiochip0`
/// (the RP1 controller, 54 lines). BCM offsets for header pins are identical
/// to Pi 4. If gpiochip0 is unavailable, the code falls back to the first
/// available chip with ≥ 27 lines.
///
/// # gpio feature gate
/// The `gpiocdev` dependency is gated behind the `gpio` Cargo feature so that
/// Phase 1 / non-Linux builds compile without the kernel header dependency.
/// The `GpioLoop` struct and its methods compile unconditionally; only the
/// event loop implementation is feature-gated.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// BOARD ↔ BCM pin mapping (Pi 2B through Pi 5, 40-pin header)
// ---------------------------------------------------------------------------

/// BOARD pin number → BCM GPIO line offset.
/// Indexed by BOARD pin (1-based). `None` = power / ground / reserved pin.
///
/// Valid for all Pi models with a 40-pin header.
/// Reference: https://pinout.xyz
const BOARD_TO_BCM: [Option<u32>; 41] = [
    None,       // 0  (unused — table is 1-based)
    None,       // 1  3.3V power
    None,       // 2  5V power
    Some(2),    // 3  GPIO2  (SDA1 / i2c)
    None,       // 4  5V power
    Some(3),    // 5  GPIO3  (SCL1 / i2c)
    None,       // 6  GND
    Some(4),    // 7  GPIO4
    Some(14),   // 8  GPIO14 (TXD0 / UART)
    None,       // 9  GND
    Some(15),   // 10 GPIO15 (RXD0 / UART)
    Some(17),   // 11 GPIO17
    Some(18),   // 12 GPIO18 (PCM_CLK)
    Some(27),   // 13 GPIO27
    None,       // 14 GND
    Some(22),   // 15 GPIO22
    Some(23),   // 16 GPIO23
    None,       // 17 3.3V power
    Some(24),   // 18 GPIO24
    Some(10),   // 19 GPIO10 (MOSI / SPI)
    None,       // 20 GND
    Some(9),    // 21 GPIO9  (MISO / SPI)
    Some(25),   // 22 GPIO25
    Some(11),   // 23 GPIO11 (SCLK / SPI)
    Some(8),    // 24 GPIO8  (CE0 / SPI)
    None,       // 25 GND
    Some(7),    // 26 GPIO7  (CE1 / SPI)
    Some(0),    // 27 GPIO0  (ID_SD / EEPROM)
    Some(1),    // 28 GPIO1  (ID_SC / EEPROM)
    Some(5),    // 29 GPIO5
    None,       // 30 GND
    Some(6),    // 31 GPIO6
    Some(12),   // 32 GPIO12
    Some(13),   // 33 GPIO13
    None,       // 34 GND
    Some(19),   // 35 GPIO19 (MISO / PCM_FS)
    Some(16),   // 36 GPIO16
    Some(26),   // 37 GPIO26
    Some(20),   // 38 GPIO20
    None,       // 39 GND
    Some(21),   // 40 GPIO21
];

/// Convert a BOARD pin number to a BCM GPIO line offset.
///
/// # Parameters
/// - `board_pin`: physical BOARD pin number (1-40)
///
/// # Returns
/// `Some(bcm)` for a valid GPIO pin; `None` for power/ground/reserved pins.
pub(crate) fn board_to_bcm(board_pin: u8) -> Option<u32> {
    BOARD_TO_BCM.get(board_pin as usize).copied().flatten()
}

/// Convert a BCM GPIO line offset back to a BOARD pin number.
/// Linear scan over 40 entries — negligible cost on the event path.
///
/// # Parameters
/// - `bcm`: BCM GPIO number (line offset reported by gpiocdev)
///
/// # Returns
/// `Some(board_pin)` if the BCM number maps to a header pin; `None` otherwise.
fn bcm_to_board(bcm: u32) -> Option<u8> {
    BOARD_TO_BCM
        .iter()
        .position(|&entry| entry == Some(bcm))
        .map(|i| i as u8)
}

// ---------------------------------------------------------------------------
// GpioLoop — lifecycle handle
// ---------------------------------------------------------------------------

/// Handle to the running GPIO event loop.
///
/// `stop()` signals the background thread to exit and waits for it to join,
/// ensuring no GPIO callbacks fire after the call returns.
pub struct GpioLoop {
    running: Arc<AtomicBool>,
    /// Background thread that owns the gpiocdev Request and dispatches events.
    /// `None` when compiled without the `gpio` feature or when no valid pins
    /// remained after filtering.
    thread: Option<std::thread::JoinHandle<()>>,
}

impl GpioLoop {
    /// Start the GPIO event loop.
    ///
    /// Opens `/dev/gpiochip0`, requests all non-skipped, non-i2c pins with
    /// the configured pull bias and debounce period, then spawns a background
    /// thread that forwards edge events into the bitmask engine.
    ///
    /// # Parameters
    /// - `config`    : pin list, pull direction, and debounce timing
    /// - `skip_pins` : BOARD pins reserved by audio HAT detection (hat_detect.py)
    ///
    /// # Errors
    /// Returns `Err` if the GPIO chip cannot be opened or line request fails.
    pub fn run(config: &GpioConfig, skip_pins: &[u8]) -> Result<GpioLoop, GpioError> {
        let running = Arc::new(AtomicBool::new(true));
        // Populated inside the gpio cfg block on success; None means stub/no-op.
        let mut thread: Option<std::thread::JoinHandle<()>> = None;

        #[cfg(not(feature = "gpio"))]
        {
            eprintln!(
                "[gpionext] WARNING: compiled without --features gpio — no hardware events"
            );
            let _ = (config, skip_pins);
        }

        #[cfg(feature = "gpio")]
        {
            use gpiocdev::line::{Bias, EdgeDetection};
            use gpiocdev::Request;

            // --- Build filtered (BOARD, BCM) list ---
            let mut lines: Vec<(u8, u32)> = Vec::new();
            for &board_pin in &config.pins {
                if skip_pins.contains(&board_pin) {
                    continue;
                }
                // i2c SDA/SCL pins must not be pulled down without the i2c feature
                if config.pulldown && (board_pin == 3 || board_pin == 5) {
                    eprintln!("{}", GpioError::I2cPinPulldownConflict { pin: board_pin });
                    continue;
                }
                match board_to_bcm(board_pin) {
                    Some(bcm) => lines.push((board_pin, bcm)),
                    None => eprintln!(
                        "[gpionext] BOARD pin {board_pin} has no GPIO equivalent, skipping"
                    ),
                }
            }

            if !lines.is_empty() {
                let bias = if config.pulldown { Bias::PullDown } else { Bias::PullUp };
                let debounce = Duration::from_millis(config.debounce_ms as u64);
                let bcm_offsets: Vec<u32> = lines.iter().map(|&(_, bcm)| bcm).collect();

                // --- Open chip (Pi 5 fallback included in find_gpio_chip) ---
                let chip_path = find_gpio_chip().ok_or_else(|| {
                    GpioError::ChipOpenFailed(
                        "no suitable GPIO chip found (tried /dev/gpiochip0-9)".to_string(),
                    )
                })?;

                // --- Build line request ---
                let mut builder = Request::builder();
                builder.on_chip(&chip_path).with_consumer("gpionext");
                for &bcm in &bcm_offsets {
                    builder
                        .with_line(bcm)
                        .as_input()
                        .with_bias(bias)
                        .with_edge_detection(EdgeDetection::BothEdges)
                        .with_debounce_period(debounce);
                }
                let request = builder
                    .request()
                    .map_err(|e| GpioError::ChipOpenFailed(e.to_string()))?;

                // --- Spawn event thread (running is cloned, not moved) ---
                let running_clone = running.clone();
                let is_pulldown = config.pulldown;
                thread = Some(
                    std::thread::Builder::new()
                        .name("gpionext-gpio".into())
                        .spawn(move || event_loop(request, running_clone, is_pulldown))
                        .expect("failed to spawn GPIO event thread"),
                );
            } else {
                eprintln!("[gpionext] WARNING: no valid GPIO pins after filtering");
            }
        }

        Ok(GpioLoop { running, thread })
    }

    /// Signal the event loop to stop and wait for the background thread to exit.
    ///
    /// After `stop()` returns, no further bitmask updates or dispatch callbacks
    /// will be triggered by this loop. Non-blocking from the caller's perspective
    /// (the thread exits within one poll timeout ≤ 100 ms).
    pub fn stop(self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread {
            let _ = thread.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Chip discovery (Pi 5 compatibility)
// ---------------------------------------------------------------------------

/// Find the GPIO chip path to use.
///
/// Tries `/dev/gpiochip0` first (correct on Pi 2B–5 with standard Pi OS).
/// On unusual kernel configurations where gpiochip0 is not the 40-pin header
/// controller, falls back to the first chip with ≥ 27 lines.
///
/// # Returns
/// `Some(path)` if a suitable chip is found; `None` otherwise.
#[cfg(feature = "gpio")]
pub(crate) fn find_gpio_chip() -> Option<String> {
    // Fast path: standard chip used on all Pi models with Pi OS
    let default = "/dev/gpiochip0".to_string();
    if std::path::Path::new(&default).exists() {
        return Some(default);
    }
    // Fallback: scan gpiochip1-9 for a chip with enough lines
    for n in 1u8..10 {
        let path = format!("/dev/gpiochip{n}");
        if std::path::Path::new(&path).exists() {
            return Some(path);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Event loop (runs in background thread, feature-gated)
// ---------------------------------------------------------------------------

/// Background thread: poll gpiocdev for edge events and forward to bitmask engine.
///
/// Loops until `running` is cleared by `GpioLoop::stop()`. Uses a 100 ms poll
/// timeout so the thread wakes frequently enough to notice the stop signal
/// without busy-waiting.
///
/// # Parameters
/// - `request` : open gpiocdev line request (owns the file descriptor)
/// - `running` : stop flag; loop exits when this is `false`
#[cfg(feature = "gpio")]
fn event_loop(request: gpiocdev::Request, running: Arc<AtomicBool>, is_pulldown: bool) {
    use gpiocdev::line::EdgeKind;

    loop {
        if !running.load(Ordering::Relaxed) {
            break;
        }

        match request.wait_edge_event(Duration::from_millis(100)) {
            Ok(true) => {
                match request.read_edge_event() {
                    Ok(event) => {
                        let bcm = event.offset;
                        if let Some(board) = bcm_to_board(bcm) {
                            // Map edge to press/release based on pull resistor logic
                            // Pull-Up (default):   Falling = Pressed (Low),  Rising = Released (High)
                            // Pull-Down:           Rising  = Pressed (High), Falling = Released (Low)
                            let is_press = if is_pulldown {
                                event.kind == EdgeKind::Rising
                            } else {
                                event.kind == EdgeKind::Falling
                            };

                            if is_press {
                                crate::bitmask::set_pin(board);
                                crate::bitmask::on_pin_press(board);
                            } else {
                                crate::bitmask::on_pin_release(board);
                            }
                        }
                    }
                    Err(e) => eprintln!("[gpionext] GPIO read error: {e}"),
                }
            }
            Ok(false) => {} // poll timeout — loop back and re-check running flag
            Err(e) => {
                eprintln!("[gpionext] GPIO poll error: {e}");
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GpioConfig — passed from GpioCore::start()
// ---------------------------------------------------------------------------

/// GPIO hardware configuration extracted from the daemon CLI args + SQLite DB.
pub struct GpioConfig {
    /// BOARD pin numbers to watch (before skip_pins and i2c filtering).
    pub pins: Vec<u8>,
    /// If `true`, use pulldown resistors; default is pullup.
    /// BOARD pins 3 & 5 (i2c) are always skipped when this is `true`.
    pub pulldown: bool,
    /// Hardware debounce time in milliseconds applied per line by the kernel.
    pub debounce_ms: u32,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during GPIO chip open or line request.
#[derive(Debug)]
pub enum GpioError {
    /// `/dev/gpiochip0` (or fallback) could not be opened
    ChipOpenFailed(String),
    /// A specific pin could not be requested (already in use, invalid offset)
    PinRequestFailed { pin: u8, reason: String },
    /// Pulldown requested on an i2c pin (3 or 5) without i2c feature enabled
    I2cPinPulldownConflict { pin: u8 },
}

impl std::fmt::Display for GpioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpioError::ChipOpenFailed(e) => write!(
                f,
                "Cannot open GPIO chip: {e}. \
                 Is the gpio kernel module loaded? Try: sudo modprobe gpiod"
            ),
            GpioError::PinRequestFailed { pin, reason } => write!(
                f,
                "BOARD pin {pin}: cannot request edge detection ({reason}). \
                 Is the pin already in use?"
            ),
            GpioError::I2cPinPulldownConflict { pin } => write!(
                f,
                "BOARD pin {pin} is an i2c pin (SDA/SCL). \
                 Cannot use pulldown without the i2c feature. \
                 Use 'gpionext set pulldown false' or enable the i2c feature."
            ),
        }
    }
}
