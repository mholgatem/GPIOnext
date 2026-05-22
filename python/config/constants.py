"""
constants.py — Pin lists, evdev mappings, and device definitions.

No RPi.GPIO import: Pi model detection uses /proc/cpuinfo so this module
works without any GPIO library installed (safe to import on desktop too).
"""
import os

# ---------------------------------------------------------------------------
# Pi model detection (without RPi.GPIO)
# ---------------------------------------------------------------------------

def _get_pi_revision() -> int:
    """
    Read the Pi board revision from /proc/cpuinfo.
    Returns the P1 header revision (1 = original A/B 26-pin, 3 = B+ and later).

    Returns:
        int: revision number; 0 if detection fails (assume modern Pi).
    """
    try:
        with open('/proc/cpuinfo', 'r') as f:
            for line in f:
                if line.startswith('Revision'):
                    rev_str = line.split(':')[1].strip().lower()
                    rev = int(rev_str, 16)
                    # Old-style revision codes: 0002..000F = Model A/B rev 1 or 2
                    # P1 header revision 1 = 26-pin without pin 27/28 (i2c0)
                    # P1 header revision 2 = 26-pin with pin 27/28
                    # P1 header revision 3 = 40-pin (B+ and all later models)
                    if rev in (0x0002, 0x0003):
                        return 1   # original A/B, 26-pin, 9 usable GPIO
                    if 0x0004 <= rev <= 0x000F:
                        return 2   # A/B rev 2, 26-pin, more usable GPIO
                    return 3       # B+ and later, 40-pin header
    except (OSError, ValueError):
        pass
    return 3  # default: assume 40-pin (Pi 1B+ and later)


_PI_REVISION = _get_pi_revision()

# ---------------------------------------------------------------------------
# Available BOARD pin numbers by Pi model
# ---------------------------------------------------------------------------

# Pi 1 original (A/B rev 1): 9 usable GPIO pins on 26-pin header
_PINS_PI1_ORIG = (3, 5, 7, 11, 13, 15, 19, 21, 23, 8, 10, 12, 16, 18, 22, 24, 26)

# Pi 1 rev 2 / B+ / Zero / 2 / 3 / 4 / 5 — 40-pin header
_PINS_MODERN = (
    3,  5,  7, 11, 13, 15, 19, 21, 23, 29, 31, 33, 35, 37,
    8, 10, 12, 16, 18, 22, 24, 26, 32, 36, 38, 40
)

if _PI_REVISION < 3:
    AVAILABLE_PINS: tuple = _PINS_PI1_ORIG
else:
    AVAILABLE_PINS: tuple = _PINS_MODERN

AVAILABLE_PINS_STRING: str = ', '.join(map(str, AVAILABLE_PINS))

# ---------------------------------------------------------------------------
# i2c pin ID generation
# ---------------------------------------------------------------------------

def mcp23017_pin_id(address: int, port: str, bit: int) -> str:
    """
    Build the canonical string ID for an MCP23017 input pin.

    Parameters:
        address (int): i2c address of the chip (0x20-0x27)
        port    (str): 'A' or 'B'
        bit     (int): 0-7

    Returns:
        str: e.g. 'i2c-0x20-A0'
    """
    return f"i2c-0x{address:02X}-{port}{bit}"


def ads1115_pin_id(address: int, channel: int) -> str:
    """
    Build the canonical string ID for an ADS1115 analog channel.

    Parameters:
        address (int): i2c address of the chip (0x48-0x4B)
        channel (int): 0-3

    Returns:
        str: e.g. 'i2c-0x48-ch0'
    """
    return f"i2c-0x{address:02X}-ch{channel}"


def pcf8574_pin_id(address: int, pin: int) -> str:
    """
    Build the canonical string ID for a PCF8574 digital I/O pin.

    Parameters:
        address (int): i2c address of the chip (0x20-0x27)
        pin     (int): 0-7

    Returns:
        str: e.g. 'i2c-0x20-P0'
    """
    return f"i2c-0x{address:02X}-P{pin}"


def available_i2c_pins(
    mcp23017_addresses: list[int] | None = None,
    ads1115_addresses: list[int] | None = None,
    pcf8574_addresses: list[int] | None = None,
) -> list[str]:
    """
    Return a list of all i2c pin IDs for connected chips.
    Used to populate the config UI pin list alongside physical GPIO pins.

    Parameters:
        mcp23017_addresses (list[int]): detected MCP23017 i2c addresses
        ads1115_addresses  (list[int]): detected ADS1115 i2c addresses
        pcf8574_addresses  (list[int]): detected PCF8574 i2c addresses

    Returns:
        list[str]: sorted list of pin ID strings
    """
    pins: list[str] = []
    for addr in (mcp23017_addresses or []):
        for port in ('A', 'B'):
            for bit in range(8):
                pins.append(mcp23017_pin_id(addr, port, bit))
    for addr in (ads1115_addresses or []):
        for ch in range(4):
            pins.append(ads1115_pin_id(addr, ch))
    for addr in (pcf8574_addresses or []):
        for pin in range(8):
            pins.append(pcf8574_pin_id(addr, pin))
    return pins

# ---------------------------------------------------------------------------
COMMAND_PRESETS = [
    ("Volume Up", "amixer sset Master 5%+"),
    ("Volume Down", "amixer sset Master 5%-"),
    ("Volume Mute/Unmute", "amixer sset Master toggle"),
    ("Reboot System", "sudo reboot"),
    ("Shutdown System", "sudo shutdown -h now"),
    ("Kill Emulator", "killall retroarch"),
]

# Virtual device list (4 joypads + keyboard + commands)
# device_index in Rust corresponds to position in this list
# ---------------------------------------------------------------------------

DEVICE_LIST: list[str] = [
    'Joypad 1',   # device_index 0
    'Joypad 2',   # device_index 1
    'Joypad 3',   # device_index 2
    'Joypad 4',   # device_index 3
    'Keyboard',   # device_index 4
    'Commands',   # device_index 5
]

DEVICE_INDEX: dict[str, int] = {name: i for i, name in enumerate(DEVICE_LIST)}

# ---------------------------------------------------------------------------
# Joystick axis definition (EV_ABS AbsInfo)
# Used when creating virtual joypad devices in uinput.rs
# ---------------------------------------------------------------------------

# Imported lazily to avoid requiring evdev on non-Pi platforms
try:
    from evdev import AbsInfo
    JOYSTICK_AXIS = AbsInfo(value=0, min=-255, max=255, fuzz=0, flat=15, resolution=0)
except ImportError:
    JOYSTICK_AXIS = None  # uinput.rs uses the same values directly

# ---------------------------------------------------------------------------
# Gamepad button list: (display_name, evdev_code)
# ---------------------------------------------------------------------------

try:
    from evdev import ecodes as e
    BUTTON_LIST: list[tuple[str, int]] = [
        ('Start Button',           e.BTN_START),
        ('Select Button',          e.BTN_SELECT),
        ('Button A',               e.BTN_A),
        ('Button B',               e.BTN_B),
        ('Button C',               e.BTN_C),
        ('Button X',               e.BTN_X),
        ('Button Y',               e.BTN_Y),
        ('Button Z',               e.BTN_Z),
        ('Button Left Trigger 1',  e.BTN_TL),
        ('Button Right Trigger 1', e.BTN_TR),
        ('Button Left Trigger 2',  e.BTN_TL2),
        ('Button Right Trigger 2', e.BTN_TR2),
        ('Button Generic 1',       e.BTN_TRIGGER_HAPPY2),
        ('Button Generic 2',       e.BTN_TRIGGER_HAPPY3),
        ('Button Generic 3',       e.BTN_TRIGGER_HAPPY4),
        ('Button Generic 4',       e.BTN_TRIGGER_HAPPY5),
        ('Button Generic 5',       e.BTN_TRIGGER_HAPPY6),
        ('Button Generic 6',       e.BTN_TRIGGER_HAPPY7),
        ('Button Generic 7',       e.BTN_TRIGGER_HAPPY8),
        ('Button Generic 8',       e.BTN_TRIGGER_HAPPY9),
        ('Button Generic 9',       e.BTN_TRIGGER_HAPPY10),
        ('Button Generic 10',      e.BTN_TRIGGER_HAPPY11),
        ('Button Generic 11',      e.BTN_TRIGGER_HAPPY12),
        ('Button Generic 12',      e.BTN_TRIGGER_HAPPY13),
        ('Button Generic 13',      e.BTN_TRIGGER_HAPPY14),
        ('Button Generic 14',      e.BTN_TRIGGER_HAPPY15),
        ('Button Generic 15',      e.BTN_TRIGGER_HAPPY16),
        ('Button Generic 16',      e.BTN_TRIGGER_HAPPY17),
        ('Button Generic 17',      e.BTN_TRIGGER_HAPPY18),
        ('Button Generic 18',      e.BTN_TRIGGER_HAPPY19),
        ('Button Generic 19',      e.BTN_TRIGGER_HAPPY20),
        ('Button Generic 20',      e.BTN_TRIGGER_HAPPY21),
        ('Button Generic 21',      e.BTN_TRIGGER_HAPPY22),
        ('Button Generic 22',      e.BTN_TRIGGER_HAPPY23),
        ('Button Generic 23',      e.BTN_TRIGGER_HAPPY24),
    ]

    KEY_LIST: list[tuple[str, int]] = [
        ('↑ UP',               e.KEY_UP),
        ('↓ DOWN',             e.KEY_DOWN),
        ('← LEFT',             e.KEY_LEFT),
        ('→ RIGHT',            e.KEY_RIGHT),
        ('ENTER',              e.KEY_ENTER),
        ('SPACEBAR',           e.KEY_SPACE),
        ('LEFT-ALT',           e.KEY_LEFTALT),
        ('LEFT-CTRL',          e.KEY_LEFTCTRL),
        ('LEFT-SHIFT',         e.KEY_LEFTSHIFT),
        ('RIGHT-ALT',          e.KEY_RIGHTALT),
        ('RIGHT-CTRL',         e.KEY_RIGHTCTRL),
        ('RIGHT-SHIFT',        e.KEY_RIGHTSHIFT),
        ('TAB',                e.KEY_TAB),
        ('A', e.KEY_A), ('B', e.KEY_B), ('C', e.KEY_C), ('D', e.KEY_D),
        ('E', e.KEY_E), ('F', e.KEY_F), ('G', e.KEY_G), ('H', e.KEY_H),
        ('I', e.KEY_I), ('J', e.KEY_J), ('K', e.KEY_K), ('L', e.KEY_L),
        ('M', e.KEY_M), ('N', e.KEY_N), ('O', e.KEY_O), ('P', e.KEY_P),
        ('Q', e.KEY_Q), ('R', e.KEY_R), ('S', e.KEY_S), ('T', e.KEY_T),
        ('U', e.KEY_U), ('V', e.KEY_V), ('W', e.KEY_W), ('X', e.KEY_X),
        ('Y', e.KEY_Y), ('Z', e.KEY_Z),
        ('0', e.KEY_0), ('1', e.KEY_1), ('2', e.KEY_2), ('3', e.KEY_3),
        ('4', e.KEY_4), ('5', e.KEY_5), ('6', e.KEY_6), ('7', e.KEY_7),
        ('8', e.KEY_8), ('9', e.KEY_9),
        ('- MINUS',            e.KEY_MINUS),
        ("' APOSTROPHE",       e.KEY_APOSTROPHE),
        ('\\ BACKSLASH',       e.KEY_BACKSLASH),
        ('← BACKSPACE',        e.KEY_BACKSPACE),
        ('CAPSLOCK',           e.KEY_CAPSLOCK),
        (', COMMA',            e.KEY_COMMA),
        ('DELETE',             e.KEY_DELETE),
        ('. DOT',              e.KEY_DOT),
        ('END',                e.KEY_END),
        ('= EQUAL',            e.KEY_EQUAL),
        ('ESC',                e.KEY_ESC),
        ('F1',  e.KEY_F1),  ('F2',  e.KEY_F2),  ('F3',  e.KEY_F3),
        ('F4',  e.KEY_F4),  ('F5',  e.KEY_F5),  ('F6',  e.KEY_F6),
        ('F7',  e.KEY_F7),  ('F8',  e.KEY_F8),  ('F9',  e.KEY_F9),
        ('F10', e.KEY_F10), ('F11', e.KEY_F11), ('F12', e.KEY_F12),
        ('` GRAVE',            e.KEY_GRAVE),
        ('HOME',               e.KEY_HOME),
        ('INSERT',             e.KEY_INSERT),
        ('KEYPAD 0',           e.KEY_KP0),  ('KEYPAD 1', e.KEY_KP1),
        ('KEYPAD 2',           e.KEY_KP2),  ('KEYPAD 3', e.KEY_KP3),
        ('KEYPAD 4',           e.KEY_KP4),  ('KEYPAD 5', e.KEY_KP5),
        ('KEYPAD 6',           e.KEY_KP6),  ('KEYPAD 7', e.KEY_KP7),
        ('KEYPAD 8',           e.KEY_KP8),  ('KEYPAD 9', e.KEY_KP9),
        ('* KEYPAD ASTERISK',  e.KEY_KPASTERISK),
        ('KEYPAD ENTER',       e.KEY_KPENTER),
        ('= KEYPAD EQUAL',     e.KEY_KPEQUAL),
        ('- KEYPAD MINUS',     e.KEY_KPMINUS),
        ('+ KEYPAD PLUS',      e.KEY_KPPLUS),
        ('KPPLUSMINUS',        e.KEY_KPPLUSMINUS),
        ('[ LEFTBRACE',        e.KEY_LEFTBRACE),
        ('] RIGHTBRACE',       e.KEY_RIGHTBRACE),
        ('PAGEDOWN',           e.KEY_PAGEDOWN),
        ('PAGEUP',             e.KEY_PAGEUP),
        ('SCROLLLOCK',         e.KEY_SCROLLLOCK),
        ('; SEMICOLON',        e.KEY_SEMICOLON),
        ('/ SLASH',            e.KEY_SLASH),
    ]
except ImportError:
    BUTTON_LIST = []
    KEY_LIST = []
