/// config.rs — JSON-backed configuration for GPIOnext.
///
/// Replaces the SQLite config.db used by the Python implementation.
/// The JSON file is the single source of truth for all daemon settings,
/// device/button mappings, and I2C chip configuration.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ---------------------------------------------------------------------------
// Top-level config structure
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

// ---------------------------------------------------------------------------
// Daemon runtime settings (written to the systemd/S6/init service file)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonSettings {
    /// Combo window in milliseconds (multi-button hold grace period)
    pub combo_delay: u32,
    /// Key hold repeat delay in milliseconds (KEY events only)
    pub key_hold_delay: u32,
    /// GPIO debounce time in milliseconds
    pub debounce: u32,
    /// "default" or comma-separated BOARD pin list to activate
    pub pins: String,
    /// Enable internal pull-down resistors on input pins
    pub pulldown: bool,
    /// Dev mode: relaxed permission checks
    pub dev: bool,
    /// Verbose debug logging
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

// ---------------------------------------------------------------------------
// Peripheral mapping row (mirrors the GPIOnext SQLite table schema)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceRow {
    /// Virtual device: "Joypad 1".."Joypad 4", "Keyboard", "Commands"
    pub device: String,
    /// Friendly name / button label (e.g. "Button A", "DPAD 1 UP")
    pub name: String,
    /// Event type: BUTTON | KEY | AXIS | COMMAND
    #[serde(rename = "type")]
    pub event_type: String,
    /// evdev code (BUTTON/KEY), axis tuple "(3,0,-255)" (AXIS), or shell cmd (COMMAND)
    pub command: String,
    /// Pin spec: single "11", combo "(11,13)", or I2C "i2c-0x20-A0"
    pub pins: String,
}

impl DeviceRow {
    pub fn new(
        device: impl Into<String>,
        name: impl Into<String>,
        event_type: impl Into<String>,
        command: impl Into<String>,
        pins: impl Into<String>,
    ) -> Self {
        Self {
            device: device.into(),
            name: name.into(),
            event_type: event_type.into(),
            command: command.into(),
            pins: pins.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// I2C chip configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct I2cConfig {
    pub mcp23017: Vec<Mcp23017Entry>,
    pub ads1115: Vec<Ads1115Entry>,
    pub pcf8574: Vec<Pcf8574Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mcp23017Entry {
    pub bus: u8,
    /// Hex string e.g. "0x20"
    pub address: String,
    /// BOARD pin number of the interrupt line, or "" if not used
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

/// Load a GpioConfig from a JSON file. Creates a default config if the file
/// does not exist.
pub fn load(path: &Path) -> Result<GpioConfig> {
    let data =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let cfg: GpioConfig =
        serde_json::from_str(&data).with_context(|| format!("parsing {}", path.display()))?;
    Ok(cfg)
}

/// Atomically write a GpioConfig to a JSON file (write-then-rename).
pub fn save(path: &Path, cfg: &GpioConfig) -> Result<()> {
    let json = serde_json::to_string_pretty(cfg).context("serialising config")?;
    // Write to a sibling temp file then rename to avoid corruption on crash
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
// CRUD helpers
// ---------------------------------------------------------------------------

/// Add or replace a mapping row. Uniqueness key: (device, name).
pub fn upsert_mapping(cfg: &mut GpioConfig, row: DeviceRow) {
    if let Some(existing) = cfg
        .devices
        .iter_mut()
        .find(|r| r.device == row.device && r.name == row.name)
    {
        *existing = row;
    } else {
        cfg.devices.push(row);
    }
}

/// Remove all mappings for a given device + name pair.
pub fn delete_mapping(cfg: &mut GpioConfig, device: &str, name: &str) {
    cfg.devices
        .retain(|r| !(r.device == device && r.name == name));
}

/// Remove all mappings belonging to a device.
pub fn delete_device(cfg: &mut GpioConfig, device: &str) {
    cfg.devices.retain(|r| r.device != device);
}

/// Return all mappings for a given device, preserving insertion order.
pub fn get_device_rows<'a>(cfg: &'a GpioConfig, device: &str) -> Vec<&'a DeviceRow> {
    cfg.devices.iter().filter(|r| r.device == device).collect()
}

/// Return deduplicated list of device names that have at least one mapping.
pub fn active_devices(cfg: &GpioConfig) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    cfg.devices
        .iter()
        .filter_map(|r| {
            if seen.insert(r.device.clone()) {
                Some(r.device.clone())
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Pin parsing helpers (mirrors SQL.parse_pins_value / pin_value_to_vpin)
// ---------------------------------------------------------------------------

/// A parsed pin specification from the `pins` field.
#[derive(Debug, Clone, PartialEq)]
pub enum PinSpec {
    Single(u8),
    Combo(Vec<u8>),
    I2c(String),
}

/// Parse the string representation of a `pins` field into a PinSpec.
///
/// Formats accepted:
/// - `"11"`          → Single(11)
/// - `"(11, 13)"`    → Combo([11, 13])
/// - `"i2c-0x20-A0"` → I2c("i2c-0x20-A0")
pub fn parse_pins(s: &str) -> PinSpec {
    let s = s.trim();
    if s.starts_with("i2c-") {
        return PinSpec::I2c(s.to_owned());
    }
    if s.starts_with('(') {
        // Strip parens, split on commas
        let inner = s.trim_matches(|c| c == '(' || c == ')');
        let nums: Vec<u8> = inner
            .split(',')
            .filter_map(|t| t.trim().parse().ok())
            .collect();
        if nums.len() == 1 {
            return PinSpec::Single(nums[0]);
        }
        return PinSpec::Combo(nums);
    }
    if let Ok(n) = s.parse::<u8>() {
        return PinSpec::Single(n);
    }
    // Fallback: treat as I2C string
    PinSpec::I2c(s.to_owned())
}

/// Map a pin string to a virtual pin number (0-255) matching the Python
/// `pin_value_to_vpin()` convention used by the Rust core.
///
/// BOARD pins 0-63 map directly. I2C virtual pins occupy:
/// - MCP23017 `i2c-0xAA-{A|B}N`: 64 + (addr_offset * 16) + (port * 8) + bit
/// - ADS1115  `i2c-0xAA-chN`:    128 + (addr_offset * 4)  + channel
/// - PCF8574  `i2c-0xAA-PN`:     192 + (addr_offset * 8)  + pin
pub fn pin_to_vpin(s: &str) -> Option<u8> {
    match parse_pins(s) {
        PinSpec::Single(n) => Some(n),
        PinSpec::I2c(ref id) => parse_i2c_vpin(id),
        PinSpec::Combo(_) => None, // combos don't have a single vpin
    }
}

fn parse_i2c_vpin(id: &str) -> Option<u8> {
    // id format: "i2c-0xAA-{A|B}N" / "i2c-0xAA-chN" / "i2c-0xAA-PN"
    let parts: Vec<&str> = id.split('-').collect();
    if parts.len() < 3 {
        return None;
    }
    // parts[0] = "i2c", parts[1] = "0xAA", parts[2] = port/channel/pin label
    let addr = u8::from_str_radix(parts[1].trim_start_matches("0x"), 16).ok()?;
    let label = parts[2];

    if label.starts_with('A') || label.starts_with('B') {
        // MCP23017
        let port_offset: u8 = if label.starts_with('A') { 0 } else { 8 };
        let bit: u8 = label[1..].parse().ok()?;
        let addr_offset = addr.saturating_sub(0x20);
        64u8.checked_add(addr_offset.saturating_mul(16))
            .and_then(|v| v.checked_add(port_offset))
            .and_then(|v| v.checked_add(bit))
    } else if label.starts_with("ch") {
        // ADS1115
        let ch: u8 = label[2..].parse().ok()?;
        let addr_offset = addr.saturating_sub(0x48);
        128u8.checked_add(addr_offset.saturating_mul(4))
            .and_then(|v| v.checked_add(ch))
    } else if label.starts_with('P') {
        // PCF8574
        let pin: u8 = label[1..].parse().ok()?;
        let addr_offset = addr.saturating_sub(0x20);
        192u8.checked_add(addr_offset.saturating_mul(8))
            .and_then(|v| v.checked_add(pin))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn round_trip() {
        let mut cfg = GpioConfig::default();
        upsert_mapping(
            &mut cfg,
            DeviceRow::new("Joypad 1", "Button A", "BUTTON", "304", "11"),
        );
        upsert_mapping(
            &mut cfg,
            DeviceRow::new("Joypad 1", "Button B", "BUTTON", "305", "13"),
        );

        let tmp = tempfile_path();
        save(&tmp, &cfg).unwrap();
        let loaded = load(&tmp).unwrap();
        assert_eq!(loaded.devices.len(), 2);
        assert_eq!(loaded.devices[0].name, "Button A");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn upsert_replaces() {
        let mut cfg = GpioConfig::default();
        upsert_mapping(&mut cfg, DeviceRow::new("Joypad 1", "Button A", "BUTTON", "304", "11"));
        upsert_mapping(&mut cfg, DeviceRow::new("Joypad 1", "Button A", "BUTTON", "304", "15"));
        assert_eq!(cfg.devices.len(), 1);
        assert_eq!(cfg.devices[0].pins, "15");
    }

    #[test]
    fn delete_mapping_works() {
        let mut cfg = GpioConfig::default();
        upsert_mapping(&mut cfg, DeviceRow::new("Joypad 1", "Button A", "BUTTON", "304", "11"));
        upsert_mapping(&mut cfg, DeviceRow::new("Joypad 1", "Button B", "BUTTON", "305", "13"));
        delete_mapping(&mut cfg, "Joypad 1", "Button A");
        assert_eq!(cfg.devices.len(), 1);
        assert_eq!(cfg.devices[0].name, "Button B");
    }

    #[test]
    fn parse_pins_variants() {
        assert_eq!(parse_pins("11"), PinSpec::Single(11));
        assert_eq!(parse_pins("(11, 13)"), PinSpec::Combo(vec![11, 13]));
        assert_eq!(parse_pins("i2c-0x20-A0"), PinSpec::I2c("i2c-0x20-A0".into()));
    }

    #[test]
    fn pin_to_vpin_board() {
        assert_eq!(pin_to_vpin("11"), Some(11));
    }

    #[test]
    fn pin_to_vpin_mcp23017() {
        // addr=0x20 (offset 0), port A, bit 0 → 64
        assert_eq!(pin_to_vpin("i2c-0x20-A0"), Some(64));
        // addr=0x20, port B, bit 3 → 64+8+3 = 75
        assert_eq!(pin_to_vpin("i2c-0x20-B3"), Some(75));
    }

    #[test]
    fn pin_to_vpin_ads1115() {
        // addr=0x48 (offset 0), channel 2 → 128+2 = 130
        assert_eq!(pin_to_vpin("i2c-0x48-ch2"), Some(130));
    }

    fn tempfile_path() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("gpionext_test_{}.json", std::process::id()));
        p
    }
}
