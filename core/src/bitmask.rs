/// Combo resolution engine and global pin-state tracker.
///
/// # Thread explosion fix
/// # based on issue: https://github.com/mholgatem/GPIOnext/issues/91
/// Original code root cause: 'Key.release()' created a new threading.Timer
/// on every call. At 30Hz key repeat over 5+ minutes that's ~9 000 timer
/// objects (Python threads). Combined with one `threading.Timer` per combo
/// window, thread limits are exhausted.
///
/// This module fixes that:
/// 1. Combo windows: a fixed Rayon pool + generation counter replaces
///    per-window threads. Only 8 workers ever exist; tasks are queued not spawned.
/// 2. Key hold: a Rayon task loops on `is_pressed` — no new thread on release.
///
/// # Architecture
/// ```text
/// GPIO interrupt (gpio.rs)
///   → set_pin / clear_pin  (update GLOBAL_BITMASK atomically)
///   → on press: schedule_combo_resolution(device_idx, delay_ms)
///   → on release: trigger_release(device_idx, pin_mask)
///
/// Rayon task (combo window):
///   sleep(combo_delay_ms)
///   if generation unchanged → dispatch longest-matching peripheral
///
/// Rayon task (key hold):
///   sleep(key_hold_delay_ms)
///   while is_pressed && generation unchanged:
///     write EV_KEY repeat, sleep(30ms)
/// ```
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::RwLock;
use rayon::{ThreadPool, ThreadPoolBuilder};

// ---------------------------------------------------------------------------
// Configuration types
// ---------------------------------------------------------------------------

/// The action fired when a peripheral is triggered.
#[derive(Debug, Clone)]
pub enum EventType {
    /// Joystick button: send EV_KEY press + release
    Button { evdev_code: u32 },
    /// Keyboard key: send EV_KEY press, then repeat at 30 Hz after hold_delay,
    /// then release when pin clears
    Key { evdev_code: u32 },
    /// Joystick axis: send EV_ABS to the given (axis_code, value) on press,
    /// return to zero on release
    Axis {
        evdev_type: u32,
        evdev_code: u32,
        press_value: i32,
    },
    /// Shell command executed via `/bin/bash -c`; no uinput event
    Command { bash: String },
}

/// A single button / key / axis / command mapping.
///
/// Multiple pins in `pin_mask` means this is a combo (all must be pressed).
/// When two peripherals match the same bitmask, the one with more pins wins
/// (longest-match rule, same as reference code).
#[derive(Debug)]
pub struct Peripheral {
    /// Human-readable name from config DB (e.g. "START", "LEFT", "volume_up")
    pub name: String,
    /// Index into the devices array (0-3 joypads, 4 keyboard, 5 commands)
    pub device_index: usize,
    /// Bitmask of virtual BOARD pin numbers that must all be pressed to trigger this.
    /// Bit N set means virtual BOARD pin N must be held.
    pub pin_mask: [u64; 4],
    /// Number of set bits in `pin_mask`. Higher count wins combo conflicts.
    pub pin_count: u8,
    /// What to do when triggered
    pub event_type: EventType,
    /// True while this peripheral's action is active (between press and release)
    pub is_pressed: AtomicBool,
    /// Incremented on each hold-task spawn so stale hold tasks self-cancel
    pub hold_generation: AtomicU64,
}

impl Peripheral {
    /// True when every pin in `pin_mask` is set in `gpio_bitmask`.
    /// Used by combo matching to filter candidates.
    ///
    /// # Parameters
    /// - `gpio_bitmask`: current value of `GLOBAL_BITMASK`
    pub fn bitmask_in(&self, gpio_bitmask: [u64; 4]) -> bool {
        (gpio_bitmask[0] & self.pin_mask[0] == self.pin_mask[0])
            && (gpio_bitmask[1] & self.pin_mask[1] == self.pin_mask[1])
            && (gpio_bitmask[2] & self.pin_mask[2] == self.pin_mask[2])
            && (gpio_bitmask[3] & self.pin_mask[3] == self.pin_mask[3])
    }
}

/// Runtime configuration loaded from SQLite at daemon start (or reload).
pub struct Config {
    /// All peripherals across all devices, sorted longest pin_count first.
    /// Longest-first ordering is applied once here so the hot path is a
    /// simple linear scan that returns the first match.
    pub peripherals: Vec<Arc<Peripheral>>,
    /// Per-device peripheral lists (same Arc refs as `peripherals`).
    /// Index: 0-3 joypads, 4 keyboard, 5 commands.
    pub device_peripherals: [Vec<Arc<Peripheral>>; 6],
    /// Map from BOARD pin number → peripherals that include that pin.
    /// Allows the GPIO callback to quickly find only relevant peripherals
    /// without scanning the full list.
    pub pin_map: HashMap<u8, Vec<Arc<Peripheral>>>,
    /// Milliseconds to wait for additional presses before resolving a combo.
    pub combo_delay_ms: u64,
    /// Milliseconds before keyboard key starts repeating when held.
    pub key_hold_delay_ms: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Bitmask of currently-pressed BOARD pins. Bit N set = pin N is held down.
/// Updated atomically by GPIO callbacks from gpio.rs and i2c.rs.
static GLOBAL_BITMASK: [AtomicU64; 4] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Per-device combo generation counter. Incremented each time a new press
/// event starts a fresh combo window for that device. Rayon tasks capture
/// the generation at spawn time and self-cancel if it has advanced.
static COMBO_GENS: [AtomicU64; 6] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Active configuration, set by `GpioCore::start()` and replaced on
/// `GpioCore::reload()`. `None` before daemon start.
static CONFIG: OnceLock<RwLock<Option<Arc<Config>>>> = OnceLock::new();

/// Fixed-size Rayon thread pool. Initialised once; never grows.
/// Handles both combo resolution tasks and key hold tasks.
static POOL: OnceLock<Arc<ThreadPool>> = OnceLock::new();

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

/// Returns the global Rayon pool if it has been initialised, or `None`.
/// Used by uinput.rs to spawn key hold tasks without holding a reference
/// to the pool in each Peripheral.
///
/// # Returns
/// `Some(Arc<ThreadPool>)` after `init_pool()` has been called; `None` before.
pub fn get_pool() -> Option<Arc<ThreadPool>> {
    POOL.get().cloned()
}

/// Initialise (or return) the global Rayon thread pool.
///
/// # Parameters
/// - `num_threads`: pool size; 8 covers 4 joypads × potential simultaneous
///   combo + 4 concurrent key holds with headroom. Pass 0 for Rayon default.
///
/// # Returns
/// Shared reference to the pool. Safe to call multiple times.
pub fn init_pool(num_threads: usize) -> Arc<ThreadPool> {
    POOL.get_or_init(|| {
        let pool = ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .thread_name(|i| format!("gpionext-worker-{i}"))
            .build()
            .expect("failed to build Rayon pool");
        Arc::new(pool)
    })
    .clone()
}

/// Install a new (or reloaded) config from an existing Arc.
pub fn set_config_arc(config: Arc<Config>) {
    let lock = CONFIG.get_or_init(|| RwLock::new(None));
    *lock.write() = Some(config);
}

/// Install a new (or reloaded) config. Call this from `GpioCore::start()`
/// and `GpioCore::reload()`.
///
/// # Parameters
/// - `config`: fully constructed `Config` from the Python-side SQLite load
pub fn set_config(config: Config) {
    set_config_arc(Arc::new(config));
}

// ---------------------------------------------------------------------------
// Pin state helpers — called by gpio.rs on each GPIO edge event
// ---------------------------------------------------------------------------

/// Returns the current pressed-pin bitmask.
/// Exposed to Python via `lib.rs` for the live pin monitor UI.
///
/// # Returns
/// `[u64; 4]` where bit N in word i is set when BOARD pin (i*64 + N) is currently held.
pub fn current_bitmask() -> [u64; 4] {
    [
        GLOBAL_BITMASK[0].load(Ordering::Relaxed),
        GLOBAL_BITMASK[1].load(Ordering::Relaxed),
        GLOBAL_BITMASK[2].load(Ordering::Relaxed),
        GLOBAL_BITMASK[3].load(Ordering::Relaxed),
    ]
}

/// Mark a pin as pressed (rising edge). Updates `GLOBAL_BITMASK` atomically.
///
/// # Parameters
/// - `board_pin`: physical or virtual BOARD pin number (1-255)
pub fn set_pin(board_pin: u8) {
    let idx = (board_pin / 64) as usize;
    if idx < 4 {
        GLOBAL_BITMASK[idx].fetch_or(1u64 << (board_pin % 64), Ordering::SeqCst);
    }
}

/// Mark a pin as released (falling edge). Updates `GLOBAL_BITMASK` atomically.
///
/// # Parameters
/// - `board_pin`: physical or virtual BOARD pin number (1-255)
pub fn clear_pin(board_pin: u8) {
    let idx = (board_pin / 64) as usize;
    if idx < 4 {
        GLOBAL_BITMASK[idx].fetch_and(!(1u64 << (board_pin % 64)), Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Combo resolution — called by gpio.rs after set_pin()
// ---------------------------------------------------------------------------

/// Schedule a combo resolution task for every device that has a peripheral
/// whose `pin_mask` includes `board_pin`.
///
/// A generation counter acts as a cancellation token: if another press event
/// arrives during the combo window, the old generation is stale and its task
/// exits without dispatching.
///
/// # Parameters
/// - `board_pin`: the pin that just fired (used to look up affected devices)
pub fn on_pin_press(board_pin: u8) {
    let config_lock = match CONFIG.get() {
        Some(l) => l,
        None => return,
    };
    let config = match config_lock.read().as_ref() {
        Some(c) => c.clone(),
        None => return,
    };

    let combo_delay = config.combo_delay_ms;

    // Determine which device indices are affected by this pin
    let affected_devices: Vec<usize> = config
        .pin_map
        .get(&board_pin)
        .map(|pvec| {
            pvec.iter()
                .map(|p| p.device_index)
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default();

    let pool = match POOL.get() {
        Some(p) => p.clone(),
        None => return,
    };

    for device_index in affected_devices {
        // Bump generation; capture new value for this task
        let gen = COMBO_GENS[device_index].fetch_add(1, Ordering::SeqCst) + 1;
        let cfg = config.clone();

        pool.spawn(move || {
            // Wait out the combo window
            std::thread::sleep(Duration::from_millis(combo_delay));

            // Bail out if a newer press superseded this window
            if COMBO_GENS[device_index].load(Ordering::SeqCst) != gen {
                return;
            }

            let bitmask = [
                GLOBAL_BITMASK[0].load(Ordering::SeqCst),
                GLOBAL_BITMASK[1].load(Ordering::SeqCst),
                GLOBAL_BITMASK[2].load(Ordering::SeqCst),
                GLOBAL_BITMASK[3].load(Ordering::SeqCst),
            ];
            resolve_and_dispatch(&cfg, device_index, bitmask);
        });
    }
}

/// Handle a pin release: clear the bitmask and trigger release on any
/// peripheral that is currently marked as pressed.
///
/// # Parameters
/// - `board_pin`: the pin that was just released
pub fn on_pin_release(board_pin: u8) {
    clear_pin(board_pin);

    let config_lock = match CONFIG.get() {
        Some(l) => l,
        None => return,
    };
    let config = match config_lock.read().as_ref() {
        Some(c) => c.clone(),
        None => return,
    };

    // Release any peripheral that uses this pin AND is currently active
    if let Some(candidates) = config.pin_map.get(&board_pin) {
        for peripheral in candidates {
            if peripheral.is_pressed.load(Ordering::Relaxed) {
                crate::uinput::dispatch_release(peripheral);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Combo matching — runs inside the Rayon task after combo_delay
// ---------------------------------------------------------------------------

/// Find the longest-matching peripheral for `device_index` in `bitmask` and
/// dispatch a press event. "Longest" = most pins in `pin_mask` (combos win
/// over single-pin mappings when both match).
///
/// Iterating `device_peripherals[device_index]` in longest-first order
/// (guaranteed by `Config` construction) means the first `bitmask_in` match
/// is the correct one.
///
/// # Parameters
/// - `config`       : current configuration snapshot
/// - `device_index` : which device's peripherals to scan (0-5)
/// - `bitmask`      : current GLOBAL_BITMASK snapshot
fn resolve_and_dispatch(config: &Arc<Config>, device_index: usize, bitmask: [u64; 4]) {
    for peripheral in &config.device_peripherals[device_index] {
        if peripheral.bitmask_in(bitmask) {
            crate::uinput::dispatch_press(peripheral, config.key_hold_delay_ms);
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Config construction helpers (called from lib.rs when parsing Python dict)
// ---------------------------------------------------------------------------

/// Build a `Config` from raw vectors of peripheral data extracted from Python.
/// Sorts peripherals longest-first and builds the pin_map index.
///
/// # Parameters
/// - `peripherals`      : all peripherals parsed from the Python config dict
/// - `combo_delay_ms`   : from `args.combo_delay`
/// - `key_hold_delay_ms`: from `args.key_hold_delay` (default 350)
///
/// # Returns
/// A fully constructed `Config` ready for `set_config()`.
pub fn build_config(
    peripherals: Vec<Peripheral>,
    combo_delay_ms: u64,
    key_hold_delay_ms: u64,
) -> Config {
    // Wrap in Arc so the same Peripheral can appear in multiple indices
    let mut arced: Vec<Arc<Peripheral>> = peripherals.into_iter().map(Arc::new).collect();

    // Sort longest pin_count first: combos take priority over single pins
    arced.sort_by(|a, b| b.pin_count.cmp(&a.pin_count));

    // Build per-device lists (same sort order preserved)
    let mut device_peripherals: [Vec<Arc<Peripheral>>; 6] = Default::default();
    for p in &arced {
        device_peripherals[p.device_index].push(p.clone());
    }

    // Build pin → peripheral map for fast GPIO callback lookup
    let mut pin_map: HashMap<u8, Vec<Arc<Peripheral>>> = HashMap::new();
    for p in &arced {
        for i in 0..4 {
            if p.pin_mask[i] == 0 {
                continue;
            }
            for bit in 0u8..64 {
                if p.pin_mask[i] & (1u64 << bit) != 0 {
                    pin_map
                        .entry((i as u8 * 64) + bit)
                        .or_default()
                        .push(p.clone());
                }
            }
        }
    }

    Config {
        peripherals: arced,
        device_peripherals,
        pin_map,
        combo_delay_ms,
        key_hold_delay_ms,
    }
}
