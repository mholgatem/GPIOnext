/// config.rs — JSON config loading and Peripheral construction for the Rust daemon.
///
/// The JSON schema is identical to the one written by `gpionext-config` so that
/// a single `gpionext.json` file is the shared source of truth for both tools.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, AtomicU64},
};

use gpionext_core::bitmask::{EventType, Peripheral};

// ---------------------------------------------------------------------------
// JSON schema (mirrors config-manager/src/config.rs)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpioConfig {
    pub version: u32,
    pub daemon: DaemonSettings,
    pub devices: Vec<DeviceRow>,
    pub i2c: I2cConfig,
}

impl Default for GpioConfig {
    fn default() -> Self {
        Self {
            version: 1,
            daemon: DaemonSettings::default(),
            devices: Vec::new(),
            i2c: I2cConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonSettings {
    pub combo_delay: u32,
    pub key_hold_delay: u32,
    pub debounce: u32,
    /// "default" or comma-separated BOARD pin numbers
    pub pins: String,
    pub pulldown: bool,
    pub dev: bool,
    pub debug: bool,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            combo_delay: 50,
            key_hold_delay: 350,
            debounce: 1,
            pins: "default".into(),
            pulldown: false, // pull-up is standard: pin held high, button connects to GND
            dev: false,
            debug: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceRow {
    pub device: String,
    pub name: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub command: String,
    pub pins: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct I2cConfig {
    pub mcp23017: Vec<Mcp23017Entry>,
    pub ads1115: Vec<Ads1115Entry>,
    pub pcf8574: Vec<Pcf8574Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mcp23017Entry {
    pub bus: u8,
    pub address: String,
    pub int_pin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ads1115Entry {
    pub bus: u8,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pcf8574Entry {
    pub bus: u8,
    pub address: String,
    pub int_pin: String,
}

// ---------------------------------------------------------------------------
// Load / save
// ---------------------------------------------------------------------------

/// Load a `GpioConfig` from a JSON file. Returns `Ok(Default)` if the file
/// does not exist (daemon can start with empty config and wait for
/// `gpionext-config` to populate it).
pub fn load(path: &Path) -> Result<GpioConfig> {
    if !path.exists() {
        return Ok(GpioConfig::default());
    }
    let data =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let cfg: GpioConfig =
        serde_json::from_str(&data).with_context(|| format!("parsing {}", path.display()))?;
    Ok(cfg)
}

/// Atomically write the config back (write-then-rename).
pub fn save(path: &Path, cfg: &GpioConfig) -> Result<()> {
    let json = serde_json::to_string_pretty(cfg).context("serialising config")?;
    let tmp = path.with_extension("json.tmp");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating dir {}", parent.display()))?;
    }
    std::fs::write(&tmp, &json).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Peripheral builder — JSON DeviceRow → core::bitmask::Peripheral
// ---------------------------------------------------------------------------

/// Device name → virtual device index used by the uinput layer.
///
/// Matches the mapping in the Python SQL.py / config_manager:
/// Joypad 1-4 → 0-3, Keyboard → 4, Commands → 5.
fn device_index(device: &str) -> usize {
    match device {
        "Joypad 1" => 0,
        "Joypad 2" => 1,
        "Joypad 3" => 2,
        "Joypad 4" => 3,
        "Keyboard" => 4,
        "Commands" => 5,
        _          => 0,
    }
}

/// Convert pin-string notation to a list of virtual pin numbers.
///
/// Formats:
/// - `"11"`          → [11]
/// - `"(11, 13)"`    → [11, 13]
/// - `"i2c-0x20-A0"` → [vpin] (I2C virtual pin, 64-255)
pub fn parse_pins_to_vpins(pins_str: &str) -> Vec<u8> {
    let s = pins_str.trim();
    if s.starts_with('(') {
        let inner = s.trim_matches(|c| c == '(' || c == ')');
        return inner
            .split(',')
            .filter_map(|t| t.trim().parse::<u8>().ok())
            .collect();
    }
    if s.starts_with("i2c-") {
        if let Some(vpin) = i2c_vpin(s) {
            return vec![vpin];
        }
        return vec![];
    }
    if let Ok(n) = s.parse::<u8>() {
        return vec![n];
    }
    vec![]
}

fn i2c_vpin(id: &str) -> Option<u8> {
    let parts: Vec<&str> = id.split('-').collect();
    if parts.len() < 3 {
        return None;
    }
    let addr = u8::from_str_radix(parts[1].trim_start_matches("0x"), 16).ok()?;
    let label = parts[2];
    if label.starts_with('A') || label.starts_with('B') {
        let port_offset: u8 = if label.starts_with('A') { 0 } else { 8 };
        let bit: u8 = label[1..].parse().ok()?;
        let addr_offset = addr.saturating_sub(0x20);
        64u8.checked_add(addr_offset.saturating_mul(16))
            .and_then(|v| v.checked_add(port_offset))
            .and_then(|v| v.checked_add(bit))
    } else if label.starts_with("ch") {
        let ch: u8 = label[2..].parse().ok()?;
        let addr_offset = addr.saturating_sub(0x48);
        128u8.checked_add(addr_offset.saturating_mul(4))
            .and_then(|v| v.checked_add(ch))
    } else if label.starts_with('P') {
        let pin: u8 = label[1..].parse().ok()?;
        let addr_offset = addr.saturating_sub(0x20);
        192u8.checked_add(addr_offset.saturating_mul(8))
            .and_then(|v| v.checked_add(pin))
    } else {
        None
    }
}

/// Parse an AXIS command string `"(evdev_type, evdev_code, value)"`.
fn parse_axis_command(s: &str, name: &str) -> Option<(u32, u32, i32)> {
    let inner = s.trim().trim_start_matches('(').trim_end_matches(')');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 {
        eprintln!("[gpionext] AXIS '{name}': malformed command '{s}'");
        return None;
    }
    let et = parts[0].trim().parse::<u32>().ok()?;
    let ec = parts[1].trim().parse::<u32>().ok()?;
    let pv = parts[2].trim().parse::<i32>().ok()?;
    Some((et, ec, pv))
}

/// Build the `Vec<Peripheral>` that `bitmask::build_config` expects from the
/// JSON device rows. Rows with unparseable types or pins are logged and skipped.
pub fn build_peripherals(cfg: &GpioConfig) -> Vec<Peripheral> {
    let mut result = Vec::new();

    for row in &cfg.devices {
        let vpins = parse_pins_to_vpins(&row.pins);
        if vpins.is_empty() {
            eprintln!("[gpionext] '{}' has no valid pins, skipping", row.name);
            continue;
        }

        let mut pin_mask: [u64; 4] = [0; 4];
        for &vpin in &vpins {
            let idx = (vpin / 64) as usize;
            if idx < 4 {
                pin_mask[idx] |= 1u64 << (vpin % 64);
            }
        }
        let pin_count = pin_mask.iter().map(|w| w.count_ones()).sum::<u32>() as u8;

        let event_type = match row.event_type.as_str() {
            "BUTTON" => match row.command.parse::<u32>() {
                Ok(code) => EventType::Button { evdev_code: code },
                Err(_) => {
                    eprintln!("[gpionext] BUTTON '{}': bad evdev code '{}'", row.name, row.command);
                    continue;
                }
            },
            "KEY" => match row.command.parse::<u32>() {
                Ok(code) => EventType::Key { evdev_code: code },
                Err(_) => {
                    eprintln!("[gpionext] KEY '{}': bad evdev code '{}'", row.name, row.command);
                    continue;
                }
            },
            "AXIS" => match parse_axis_command(&row.command, &row.name) {
                Some((et, ec, pv)) => EventType::Axis {
                    evdev_type: et,
                    evdev_code: ec,
                    press_value: pv,
                },
                None => continue,
            },
            "COMMAND" => EventType::Command { bash: row.command.clone() },
            other => {
                eprintln!("[gpionext] '{}': unknown type '{other}'", row.name);
                continue;
            }
        };

        result.push(Peripheral {
            name: row.name.clone(),
            device_index: device_index(&row.device),
            pin_mask,
            pin_count,
            event_type,
            is_pressed: AtomicBool::new(false),
            hold_generation: AtomicU64::new(0),
        });
    }

    result
}

// ---------------------------------------------------------------------------
// Active pin list resolution
// ---------------------------------------------------------------------------

/// Resolve the `pins` field in DaemonSettings to a list of BOARD pin numbers.
///
/// "default" expands to the standard GPIO-capable pins on the detected Pi model.
/// Otherwise, parses a comma-separated list.
pub fn effective_pins(settings: &DaemonSettings) -> Vec<u8> {
    if settings.pins.trim().eq_ignore_ascii_case("default") {
        default_pins()
    } else {
        settings
            .pins
            .split(',')
            .filter_map(|t| t.trim().parse::<u8>().ok())
            .collect()
    }
}

/// Default set of GPIO-capable BOARD pins (works for Pi 2B through Pi 5).
/// Excludes power/ground, I2C (3,5), and SPI pins.
fn default_pins() -> Vec<u8> {
    // BCM-capable BOARD pins on a standard 40-pin header:
    // 7,8,10,11,12,13,15,16,18,19,21,22,23,24,26,29,31,32,33,35,36,37,38,40
    vec![
        7, 8, 10, 11, 12, 13, 15, 16, 18, 19, 21, 22, 23, 24, 26,
        29, 31, 32, 33, 35, 36, 37, 38, 40,
    ]
}

// ---------------------------------------------------------------------------
// I2C entry helpers (convert hex-string address to u8)
// ---------------------------------------------------------------------------

/// Parse a hex address string like "0x20" or "0X48" to u8.
pub fn parse_hex_addr(s: &str) -> Option<u8> {
    u8::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
}

/// Parse the int_pin field: empty string or non-numeric → None.
pub fn parse_int_pin(s: &str) -> Option<u8> {
    let t = s.trim();
    if t.is_empty() { None } else { t.parse::<u8>().ok() }
}
