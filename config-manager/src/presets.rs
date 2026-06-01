/// presets.rs — Built-in HAT pin mapping presets.
///
/// Each preset contains a set of device rows ready to be inserted into
/// GpioConfig. Ported from python/ui/hat_presets.py.

use crate::config::DeviceRow;

// ---------------------------------------------------------------------------
// Preset metadata
// ---------------------------------------------------------------------------

pub struct Preset {
    pub key: &'static str,
    pub display_name: &'static str,
    /// Returns the device rows for this preset
    pub rows_fn: fn() -> Vec<DeviceRow>,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        key: "adafruit_bonnet",
        display_name: "Adafruit Retrogame Bonnet",
        rows_fn: adafruit_bonnet_rows,
    },
    Preset {
        key: "pimoroni_picade",
        display_name: "Pimoroni Picade HAT",
        rows_fn: pimoroni_picade_rows,
    },
    Preset {
        key: "generic_nes",
        display_name: "Generic NES/SNES Pinout",
        rows_fn: generic_nes_rows,
    },
];

pub fn get_preset_names() -> Vec<&'static str> {
    PRESETS.iter().map(|p| p.display_name).collect()
}

pub fn get_preset_rows(key: &str) -> Vec<DeviceRow> {
    PRESETS
        .iter()
        .find(|p| p.key == key)
        .map(|p| (p.rows_fn)())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Axis helper: produce 4 direction rows for a D-pad
// ---------------------------------------------------------------------------

fn axis_rows(device: &str, name: &str, up: u8, down: u8, left: u8, right: u8) -> Vec<DeviceRow> {
    vec![
        DeviceRow::new(device, format!("{name} UP"),    "AXIS", "(3, 1, -255)", up.to_string()),
        DeviceRow::new(device, format!("{name} DOWN"),  "AXIS", "(3, 1, 255)",  down.to_string()),
        DeviceRow::new(device, format!("{name} LEFT"),  "AXIS", "(3, 0, -255)", left.to_string()),
        DeviceRow::new(device, format!("{name} RIGHT"), "AXIS", "(3, 0, 255)",  right.to_string()),
    ]
}

fn btn(device: &str, name: &str, evdev: u16, pin: u8) -> DeviceRow {
    DeviceRow::new(device, name, "BUTTON", evdev.to_string(), pin.to_string())
}

// ---------------------------------------------------------------------------
// Adafruit Retrogame Bonnet
// D-pad + 4 face buttons + Start/Select
// ---------------------------------------------------------------------------

fn adafruit_bonnet_rows() -> Vec<DeviceRow> {
    let mut rows = axis_rows("Joypad 1", "DPAD 1", 11, 13, 29, 31);
    rows.extend([
        btn("Joypad 1", "Button A",      304, 7),
        btn("Joypad 1", "Button B",      305, 15),
        btn("Joypad 1", "Button X",      307, 33),
        btn("Joypad 1", "Button Y",      308, 35),
        btn("Joypad 1", "Start Button",  315, 37),
        btn("Joypad 1", "Select Button", 314, 16),
    ]);
    rows
}

// ---------------------------------------------------------------------------
// Pimoroni Picade HAT
// D-pad + 6 action buttons + Start/Select + coin
// ---------------------------------------------------------------------------

fn pimoroni_picade_rows() -> Vec<DeviceRow> {
    let mut rows = axis_rows("Joypad 1", "DPAD 1", 29, 31, 33, 35);
    rows.extend([
        btn("Joypad 1", "Button A",               304, 7),
        btn("Joypad 1", "Button B",               305, 11),
        btn("Joypad 1", "Button X",               307, 13),
        btn("Joypad 1", "Button Y",               308, 15),
        btn("Joypad 1", "Button Left Trigger 1",  310, 16),
        btn("Joypad 1", "Button Right Trigger 1", 311, 18),
        btn("Joypad 1", "Start Button",           315, 36),
        btn("Joypad 1", "Select Button",          314, 38),
        btn("Joypad 1", "Button Generic 1",       706, 40), // coin
    ]);
    rows
}

// ---------------------------------------------------------------------------
// Generic NES/SNES layout
// D-pad + A + B + Start + Select
// ---------------------------------------------------------------------------

fn generic_nes_rows() -> Vec<DeviceRow> {
    let mut rows = axis_rows("Joypad 1", "DPAD 1", 11, 13, 15, 19);
    rows.extend([
        btn("Joypad 1", "Button A",      304, 21),
        btn("Joypad 1", "Button B",      305, 23),
        btn("Joypad 1", "Start Button",  315, 29),
        btn("Joypad 1", "Select Button", 314, 31),
    ]);
    rows
}
