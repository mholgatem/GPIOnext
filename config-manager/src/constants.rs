/// constants.rs — Pin lists, evdev codes, and device definitions.
///
/// Pi model detection reads /proc/cpuinfo at runtime so this module works
/// on any Linux platform including x86_64 desktop (returns modern-Pi values).

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Pi model detection
// ---------------------------------------------------------------------------

fn get_pi_revision() -> u32 {
    let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") else {
        return 3; // assume 40-pin
    };
    for line in cpuinfo.lines() {
        if line.starts_with("Revision") {
            if let Some(val) = line.split(':').nth(1) {
                if let Ok(rev) = u32::from_str_radix(val.trim(), 16) {
                    if rev == 0x0002 || rev == 0x0003 {
                        return 1;
                    }
                    if (0x0004..=0x000F).contains(&rev) {
                        return 2;
                    }
                    return 3;
                }
            }
        }
    }
    3
}

// ---------------------------------------------------------------------------
// Available BOARD pin numbers by Pi model
// ---------------------------------------------------------------------------

const PINS_PI1_ORIG: &[u8] = &[3, 5, 7, 11, 13, 15, 19, 21, 23, 8, 10, 12, 16, 18, 22, 24, 26];
const PINS_MODERN: &[u8] = &[
    3, 5, 7, 11, 13, 15, 19, 21, 23, 29, 31, 33, 35, 37, 8, 10, 12, 16, 18, 22, 24, 26, 32, 36,
    38, 40,
];

pub fn available_pins() -> &'static [u8] {
    if get_pi_revision() < 3 {
        PINS_PI1_ORIG
    } else {
        PINS_MODERN
    }
}

pub fn available_pins_string() -> String {
    available_pins()
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// BOARD pin → GPIO (BCM) number
// ---------------------------------------------------------------------------

pub fn board_to_gpio() -> HashMap<u8, u8> {
    if get_pi_revision() == 1 {
        [
            (3, 0), (5, 1), (7, 4), (8, 14), (10, 15), (11, 17), (12, 18), (13, 21),
            (15, 22), (16, 23), (18, 24), (19, 10), (21, 9), (22, 25), (23, 11), (24, 8), (26, 7),
        ]
        .into()
    } else {
        [
            (3, 2), (5, 3), (7, 4), (8, 14), (10, 15), (11, 17), (12, 18), (13, 27),
            (15, 22), (16, 23), (18, 24), (19, 10), (21, 9), (22, 25), (23, 11), (24, 8), (26, 7),
            (27, 0), (28, 1),
            (29, 5), (31, 6), (32, 12), (33, 13), (35, 19), (36, 16), (37, 26), (38, 20), (40, 21),
        ]
        .into()
    }
}

// ---------------------------------------------------------------------------
// I2C pin ID helpers
// ---------------------------------------------------------------------------

pub fn mcp23017_pin_id(address: u8, port: char, bit: u8) -> String {
    format!("i2c-0x{address:02X}-{port}{bit}")
}

pub fn ads1115_pin_id(address: u8, channel: u8) -> String {
    format!("i2c-0x{address:02X}-ch{channel}")
}

pub fn pcf8574_pin_id(address: u8, pin: u8) -> String {
    format!("i2c-0x{address:02X}-P{pin}")
}

pub fn available_i2c_pins(
    mcp23017_addrs: &[u8],
    ads1115_addrs: &[u8],
    pcf8574_addrs: &[u8],
) -> Vec<String> {
    let mut pins = Vec::new();
    for &addr in mcp23017_addrs {
        for port in ['A', 'B'] {
            for bit in 0..8u8 {
                pins.push(mcp23017_pin_id(addr, port, bit));
            }
        }
    }
    for &addr in ads1115_addrs {
        for ch in 0..4u8 {
            pins.push(ads1115_pin_id(addr, ch));
        }
    }
    for &addr in pcf8574_addrs {
        for pin in 0..8u8 {
            pins.push(pcf8574_pin_id(addr, pin));
        }
    }
    pins
}

// ---------------------------------------------------------------------------
// Virtual device list
// ---------------------------------------------------------------------------

pub const DEVICE_LIST: &[&str] = &[
    "Joypad 1",
    "Joypad 2",
    "Joypad 3",
    "Joypad 4",
    "Keyboard",
    "Commands",
];

// ---------------------------------------------------------------------------
// Gamepad button list: (display_name, evdev_code)
// evdev codes are stable Linux kernel constants — no evdev crate needed.
// ---------------------------------------------------------------------------

pub const BUTTON_LIST: &[(&str, u16)] = &[
    ("Start Button",           315),
    ("Select Button",          314),
    ("Button A",               304),
    ("Button B",               305),
    ("Button C",               306),
    ("Button X",               307),
    ("Button Y",               308),
    ("Button Z",               309),
    ("Button Left Trigger 1",  310),
    ("Button Right Trigger 1", 311),
    ("Button Left Trigger 2",  312),
    ("Button Right Trigger 2", 313),
    ("Button Generic 1",       706),
    ("Button Generic 2",       707),
    ("Button Generic 3",       708),
    ("Button Generic 4",       709),
    ("Button Generic 5",       710),
    ("Button Generic 6",       711),
    ("Button Generic 7",       712),
    ("Button Generic 8",       713),
    ("Button Generic 9",       714),
    ("Button Generic 10",      715),
    ("Button Generic 11",      716),
    ("Button Generic 12",      717),
    ("Button Generic 13",      718),
    ("Button Generic 14",      719),
    ("Button Generic 15",      720),
    ("Button Generic 16",      721),
    ("Button Generic 17",      722),
    ("Button Generic 18",      723),
    ("Button Generic 19",      724),
    ("Button Generic 20",      725),
    ("Button Generic 21",      726),
    ("Button Generic 22",      727),
    ("Button Generic 23",      728),
];

// ---------------------------------------------------------------------------
// Keyboard key list: (display_name, evdev_code)
// ---------------------------------------------------------------------------

pub const KEY_LIST: &[(&str, u16)] = &[
    ("↑ UP",               103),
    ("↓ DOWN",             108),
    ("← LEFT",             105),
    ("→ RIGHT",            106),
    ("ENTER",              28),
    ("SPACEBAR",           57),
    ("LEFT-ALT",           56),
    ("LEFT-CTRL",          29),
    ("LEFT-SHIFT",         42),
    ("RIGHT-ALT",          100),
    ("RIGHT-CTRL",         97),
    ("RIGHT-SHIFT",        54),
    ("TAB",                15),
    ("A", 30), ("B", 48), ("C", 46), ("D", 32),
    ("E", 18), ("F", 33), ("G", 34), ("H", 35),
    ("I", 23), ("J", 36), ("K", 37), ("L", 38),
    ("M", 50), ("N", 49), ("O", 24), ("P", 25),
    ("Q", 16), ("R", 19), ("S", 31), ("T", 20),
    ("U", 22), ("V", 47), ("W", 17), ("X", 45),
    ("Y", 21), ("Z", 44),
    ("0", 11), ("1", 2),  ("2", 3),  ("3", 4),
    ("4", 5),  ("5", 6),  ("6", 7),  ("7", 8),
    ("8", 9),  ("9", 10),
    ("- MINUS",           12),
    ("' APOSTROPHE",      40),
    ("\\ BACKSLASH",      43),
    ("← BACKSPACE",       14),
    ("CAPSLOCK",          58),
    (", COMMA",           51),
    ("DELETE",            111),
    (". DOT",             52),
    ("END",               107),
    ("= EQUAL",           13),
    ("ESC",               1),
    ("F1",  59), ("F2",  60), ("F3",  61),
    ("F4",  62), ("F5",  63), ("F6",  64),
    ("F7",  65), ("F8",  66), ("F9",  67),
    ("F10", 68), ("F11", 87), ("F12", 88),
    ("` GRAVE",           41),
    ("HOME",              102),
    ("INSERT",            110),
    ("KEYPAD 0",          82), ("KEYPAD 1", 79),
    ("KEYPAD 2",          80), ("KEYPAD 3", 81),
    ("KEYPAD 4",          75), ("KEYPAD 5", 76),
    ("KEYPAD 6",          77), ("KEYPAD 7", 71),
    ("KEYPAD 8",          72), ("KEYPAD 9", 73),
    ("* KEYPAD ASTERISK", 55),
    ("KEYPAD ENTER",      96),
    ("= KEYPAD EQUAL",    117),
    ("- KEYPAD MINUS",    74),
    ("+ KEYPAD PLUS",     78),
    ("KPPLUSMINUS",       118),
    ("[ LEFTBRACE",       26),
    ("] RIGHTBRACE",      27),
    ("PAGEDOWN",          109),
    ("PAGEUP",            104),
    ("SCROLLLOCK",        70),
    ("; SEMICOLON",       39),
    ("/ SLASH",           53),
];

// ---------------------------------------------------------------------------
// Command presets
// ---------------------------------------------------------------------------

pub const COMMAND_PRESETS: &[(&str, &str)] = &[
    ("Volume Up",         "amixer sset Master 5%+"),
    ("Volume Down",       "amixer sset Master 5%-"),
    ("Volume Mute/Unmute","amixer sset Master toggle"),
    ("Reboot System",     "sudo reboot"),
    ("Shutdown System",   "sudo shutdown -h now"),
    ("Kill Emulator",     "killall retroarch"),
];
