"""
hat_presets.py — Built-in HAT / controller pin mapping presets.

Presets auto-fill the config tool with known pin assignments for popular
GPIO controller HATs, saving users from manually mapping every button.
Users confirm before any existing mapping is overwritten.

Preset format:
    {
        'joypad1': {
            'axes': [
                {'name': 'DPAD 1', 'UP': [pin], 'DOWN': [pin], 'LEFT': [pin], 'RIGHT': [pin]},
            ],
            'buttons': [
                {'name': 'Start Button',  'evdev': BTN_START,  'pins': [pin]},
                ...
            ]
        },
        'keyboard': { 'keys': [...] },
        'commands': { 'commands': [...] },
    }

Only the devices/buttons listed in a preset are populated; everything else
is left as-is in the database.
"""
from typing import Any

try:
    from evdev import ecodes as e
    _HAS_EVDEV = True
except ImportError:
    _HAS_EVDEV = False
    class _e:  # fallback stubs so the module loads without evdev
        BTN_START = 315; BTN_SELECT = 314
        BTN_A = 304; BTN_B = 305; BTN_X = 307; BTN_Y = 308
        BTN_TL = 310; BTN_TR = 311; BTN_TL2 = 312; BTN_TR2 = 313
        BTN_TRIGGER_HAPPY2 = 706
        KEY_UP = 103; KEY_DOWN = 108; KEY_LEFT = 105; KEY_RIGHT = 106
        KEY_ENTER = 28; KEY_ESC = 1; KEY_LEFTCTRL = 29; KEY_LEFTALT = 56
    e = _e()  # type: ignore

# ---------------------------------------------------------------------------
# Preset definitions
# ---------------------------------------------------------------------------

PRESETS: dict[str, dict[str, Any]] = {

    # -----------------------------------------------------------------------
    # Adafruit Retrogame Bonnet
    # https://www.adafruit.com/product/3422
    # GPIO pins (BCM) → BOARD conversion applied
    # BCM: 4→7, 17→11, 22→15, 27→13, 23→16→none, 5→29, 6→31, 13→33, 19→35, 26→37
    # Standard layout: D-pad + 4 face buttons + Start/Select
    # -----------------------------------------------------------------------
    'adafruit_bonnet': {
        '_display_name': 'Adafruit Retrogame Bonnet',
        'joypad1': {
            'axes': [
                {
                    'name': 'DPAD 1',
                    'UP':    [11],   # BCM 17
                    'DOWN':  [13],   # BCM 27
                    'LEFT':  [29],   # BCM 5
                    'RIGHT': [31],   # BCM 6
                }
            ],
            'buttons': [
                {'name': 'Button A',      'evdev': e.BTN_A,      'pins': [7]},   # BCM 4
                {'name': 'Button B',      'evdev': e.BTN_B,      'pins': [15]},  # BCM 22
                {'name': 'Button X',      'evdev': e.BTN_X,      'pins': [33]},  # BCM 13
                {'name': 'Button Y',      'evdev': e.BTN_Y,      'pins': [35]},  # BCM 19
                {'name': 'Start Button',  'evdev': e.BTN_START,  'pins': [37]},  # BCM 26
                {'name': 'Select Button', 'evdev': e.BTN_SELECT, 'pins': [16]},  # BCM 23 (if available)
            ],
        },
    },

    # -----------------------------------------------------------------------
    # Pimoroni Picade HAT
    # https://shop.pimoroni.com/products/picade-hat
    # Layout: 8 joystick directions + 6 action buttons + Start/Select + coin
    # -----------------------------------------------------------------------
    'pimoroni_picade': {
        '_display_name': 'Pimoroni Picade HAT',
        'joypad1': {
            'axes': [
                {
                    'name': 'DPAD 1',
                    'UP':    [29],   # BCM 5
                    'DOWN':  [31],   # BCM 6
                    'LEFT':  [33],   # BCM 13
                    'RIGHT': [35],   # BCM 19
                }
            ],
            'buttons': [
                {'name': 'Button A',               'evdev': e.BTN_A,               'pins': [7]},
                {'name': 'Button B',               'evdev': e.BTN_B,               'pins': [11]},
                {'name': 'Button X',               'evdev': e.BTN_X,               'pins': [13]},
                {'name': 'Button Y',               'evdev': e.BTN_Y,               'pins': [15]},
                {'name': 'Button Left Trigger 1',  'evdev': e.BTN_TL,              'pins': [16]},
                {'name': 'Button Right Trigger 1', 'evdev': e.BTN_TR,              'pins': [18]},
                {'name': 'Start Button',           'evdev': e.BTN_START,           'pins': [36]},
                {'name': 'Select Button',          'evdev': e.BTN_SELECT,          'pins': [38]},
                {'name': 'Button Generic 1',       'evdev': e.BTN_TRIGGER_HAPPY2,  'pins': [40]},  # coin
            ],
        },
    },

    # -----------------------------------------------------------------------
    # Generic NES layout
    # Common DIY wiring used in countless tutorials and Instructables projects.
    # 8 pins: D-pad (4) + A + B + Start + Select
    # -----------------------------------------------------------------------
    'generic_nes': {
        '_display_name': 'Generic NES/SNES Pinout',
        'joypad1': {
            'axes': [
                {
                    'name': 'DPAD 1',
                    'UP':    [11],
                    'DOWN':  [13],
                    'LEFT':  [15],
                    'RIGHT': [19],
                }
            ],
            'buttons': [
                {'name': 'Button A',      'evdev': e.BTN_A,      'pins': [21]},
                {'name': 'Button B',      'evdev': e.BTN_B,      'pins': [23]},
                {'name': 'Start Button',  'evdev': e.BTN_START,  'pins': [29]},
                {'name': 'Select Button', 'evdev': e.BTN_SELECT, 'pins': [31]},
            ],
        },
    },
}


# ---------------------------------------------------------------------------
# Accessor helpers
# ---------------------------------------------------------------------------

def get_preset_names() -> list[str]:
    """
    Return a list of preset keys suitable for display in the config UI.

    Returns:
        list[str]: e.g. ['adafruit_bonnet', 'pimoroni_picade', 'generic_nes']
    """
    return list(PRESETS.keys())


def get_display_name(preset_key: str) -> str:
    """
    Return the human-readable display name for a preset.

    Parameters:
        preset_key (str): a key from PRESETS

    Returns:
        str: display name, or the raw key if not found
    """
    return PRESETS.get(preset_key, {}).get('_display_name', preset_key)


def get_preset(preset_key: str) -> dict | None:
    """
    Return a preset dict by key, or None if not found.

    Parameters:
        preset_key (str): preset identifier

    Returns:
        dict or None
    """
    return PRESETS.get(preset_key)


def preset_to_db_rows(preset_key: str) -> list[tuple]:
    """
    Convert a preset into (device, name, type, command, pins) tuples
    ready for SQL.createDevice().

    Parameters:
        preset_key (str): preset identifier

    Returns:
        list[tuple]: DB insertion tuples; empty if preset not found.
    """
    preset = get_preset(preset_key)
    if not preset:
        return []

    rows: list[tuple] = []

    device_map = {'joypad1': 'Joypad 1', 'joypad2': 'Joypad 2',
                  'joypad3': 'Joypad 3', 'joypad4': 'Joypad 4',
                  'keyboard': 'Keyboard', 'commands': 'Commands'}

    for section_key, section_data in preset.items():
        if section_key.startswith('_'):
            continue
        device_name = device_map.get(section_key, section_key)

        # Axes → AXIS rows (UP/DOWN/LEFT/RIGHT each become a separate DB entry)
        for axis in section_data.get('axes', []):
            for direction in ('UP', 'DOWN', 'LEFT', 'RIGHT'):
                pins = axis.get(direction, [])
                if not pins:
                    continue
                # command format matches reference: "(evdev_type, evdev_code, value)"
                # EV_ABS=3; axis codes: ABS_Y=1 (UP/DOWN), ABS_X=0 (LEFT/RIGHT)
                if direction in ('UP', 'DOWN'):
                    axis_code = 1   # ABS_Y
                    value = -255 if direction == 'UP' else 255
                else:
                    axis_code = 0   # ABS_X
                    value = -255 if direction == 'LEFT' else 255
                command = f'(3, {axis_code}, {value})'
                pins_str = str(pins[0]) if len(pins) == 1 else str(tuple(pins))
                rows.append((device_name, f"{axis['name']} {direction}", 'AXIS', command, pins_str))

        # Buttons → BUTTON rows
        for btn in section_data.get('buttons', []):
            pins = btn.get('pins', [])
            pins_str = str(pins[0]) if len(pins) == 1 else str(tuple(pins))
            rows.append((device_name, btn['name'], 'BUTTON', str(btn['evdev']), pins_str))

        # Keys → KEY rows
        for key in section_data.get('keys', []):
            pins = key.get('pins', [])
            pins_str = str(pins[0]) if len(pins) == 1 else str(tuple(pins))
            rows.append((device_name, key['name'], 'KEY', str(key['evdev']), pins_str))

        # Commands → COMMAND rows
        for cmd in section_data.get('commands', []):
            pins = cmd.get('pins', [])
            pins_str = str(pins[0]) if len(pins) == 1 else str(tuple(pins))
            rows.append((device_name, cmd['name'], 'COMMAND', cmd['command'], pins_str))

    return rows
