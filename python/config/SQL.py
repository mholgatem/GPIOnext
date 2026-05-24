"""
SQL.py — SQLite CRUD for GPIOnext device/button configuration.

Schema (unchanged from reference):
    GPIOnext(
        id      INTEGER PRIMARY KEY AUTOINCREMENT,
        device  TEXT,    -- 'Joypad 1'..'Joypad 4', 'Keyboard', 'Commands'
        name    TEXT,    -- human label e.g. 'START', 'volume_up'
        type    TEXT,    -- 'BUTTON' | 'KEY' | 'AXIS' | 'COMMAND'
        command TEXT,    -- evdev code (int str) or bash string or axis tuple str
        pins    TEXT     -- single int or tuple str e.g. '11' or '(11, 13)'
    )

All functions preserve the same signatures as the reference SQL.py so existing
callers (config_manager.py, gpionext.py) work without changes.
"""
import ast
import os
import sqlite3
from typing import Any, Dict, List, Optional, Tuple, Union

# ---------------------------------------------------------------------------
# Install path — updated from /home/pi/gpionext to /opt/gpionext
# ---------------------------------------------------------------------------

INSTALL_PATH = '/opt/gpionext'
DEFAULT_DB_PATH = os.path.join(INSTALL_PATH, 'config', 'config.db')

# Module-level connection (initialised by init())
_conn = None  # type: Optional[sqlite3.Connection]
_cursor = None  # type: Optional[sqlite3.Cursor]


def _row_factory(cursor: sqlite3.Cursor, row: tuple) -> dict:
    """Convert a sqlite3 row tuple to a dict keyed by column name."""
    return {col[0]: row[i] for i, col in enumerate(cursor.description)}


# ---------------------------------------------------------------------------
# Pin value parsing / conversion helpers
# ---------------------------------------------------------------------------

def parse_pins_value(raw: str) -> List[Union[int, str]]:
    """
    Safely parse a stored DB pins value into physical BOARD pins and/or
    virtual I2C pin strings.

    Supported stored formats include:
      - "7"
      - "(7, 11)" or "[7, 11]"
      - "('7', '11')"
      - "i2c-0x20-A0" or "i2c-0x48-ch0"

    Tuple/list-style values are parsed with ast.literal_eval. eval() must not
    be used for DB pin parsing.
    """
    if raw is None:
        return []

    text = str(raw).strip()
    if not text:
        return []

    if _is_i2c_pin_string(text):
        return [text]

    try:
        return [int(text)]
    except ValueError:
        pass

    if text[0] not in '([':
        return []

    try:
        parsed = ast.literal_eval(text)
    except (SyntaxError, ValueError):
        return []

    if not isinstance(parsed, (list, tuple)):
        return []

    pins = []  # type: List[Union[int, str]]
    for item in parsed:
        pin = _normalise_pin_item(item)
        if pin is not None:
            pins.append(pin)
    return pins


def pin_value_to_vpin(pin: Union[int, str]) -> Optional[int]:
    """
    Convert a parsed physical or virtual pin value to the integer pin number
    expected by the core/runtime display layers.
    """
    if isinstance(pin, int):
        return pin
    if isinstance(pin, str):
        text = pin.strip()
        if _is_i2c_pin_string(text):
            try:
                return _map_i2c_pin_string_to_vpin(text)
            except (IndexError, ValueError):
                return None
        try:
            return int(text)
        except ValueError:
            return None
    return None


def format_pins_value(raw: str) -> str:
    """
    Format a stored pins value consistently for menus and status labels.
    Falls back to the original raw value if it cannot be parsed.
    """
    pins = parse_pins_value(raw)
    if pins:
        return ', '.join(str(pin) for pin in pins)
    return '' if raw is None else str(raw)


def _normalise_pin_item(item: object) -> Optional[Union[int, str]]:
    """Return a supported pin item or None for unsupported values."""
    if isinstance(item, int):
        return item
    if isinstance(item, str):
        text = item.strip()
        if _is_i2c_pin_string(text):
            return text
        try:
            return int(text)
        except ValueError:
            return None
    return None


def _is_i2c_pin_string(value: str) -> bool:
    """Return True for supported virtual I2C pin identifiers."""
    parts = value.split('-')
    if len(parts) != 3 or parts[0] != 'i2c':
        return False
    if not parts[1].lower().startswith('0x'):
        return False
    try:
        int(parts[1], 16)
    except ValueError:
        return False
    pin = parts[2]
    if pin.startswith('ch'):
        return pin[2:].isdigit()
    return len(pin) >= 2 and pin[0] in ('A', 'B', 'P') and pin[1:].isdigit()

# ---------------------------------------------------------------------------
# Initialisation
# ---------------------------------------------------------------------------

def init(db_path: Optional[str] = None) -> None:
    """
    Open (or create) the SQLite database and ensure the GPIOnext table exists.
    Must be called once before any other function in this module.

    Parameters:
        db_path (str|None): override the database file path. Uses
                            DEFAULT_DB_PATH when None, falling back to a
                            local ./config/config.db if /opt/gpionext is absent.
    """
    global _conn, _cursor

    if db_path is None:
        db_path = _resolve_db_path()

    os.makedirs(os.path.dirname(db_path), exist_ok=True)
    _conn = sqlite3.connect(db_path, check_same_thread=False)
    _conn.row_factory = _row_factory
    _cursor = _conn.cursor()

    _cursor.execute(
        'CREATE TABLE IF NOT EXISTS GPIOnext ('
        '  id      INTEGER PRIMARY KEY AUTOINCREMENT UNIQUE,'
        '  device  TEXT,'
        '  name    TEXT,'
        '  type    TEXT,'
        '  command TEXT,'
        '  pins    TEXT'
        ')'
    )
    _cursor.execute(
        'CREATE TABLE IF NOT EXISTS I2C_MCP23017 ('
        '  id       INTEGER PRIMARY KEY AUTOINCREMENT,'
        '  bus      INTEGER DEFAULT 1,'
        '  address  INTEGER DEFAULT 32,'
        '  int_pin  INTEGER'
        ')'
    )
    _cursor.execute(
        'CREATE TABLE IF NOT EXISTS I2C_ADS1115 ('
        '  id       INTEGER PRIMARY KEY AUTOINCREMENT,'
        '  bus      INTEGER DEFAULT 1,'
        '  address  INTEGER DEFAULT 72'
        ')'
    )
    _cursor.execute(
        'CREATE TABLE IF NOT EXISTS I2C_PCF8574 ('
        '  id       INTEGER PRIMARY KEY AUTOINCREMENT,'
        '  bus      INTEGER DEFAULT 1,'
        '  address  INTEGER DEFAULT 32,'
        '  int_pin  INTEGER'
        ')'
    )
    _conn.commit()


def _resolve_db_path() -> str:
    """
    Determine the database path.
    Prefers DEFAULT_DB_PATH (/opt/gpionext/config/config.db).
    Falls back to ./config/config.db relative to this file's location,
    which allows running from the source tree during development.

    Returns:
        str: absolute path to config.db
    """
    if os.path.isdir(os.path.dirname(DEFAULT_DB_PATH)):
        return DEFAULT_DB_PATH
    # Development fallback: config/ next to this file
    here = os.path.dirname(os.path.realpath(__file__))
    return os.path.join(here, 'config.db')


# ---------------------------------------------------------------------------
# Read operations
# ---------------------------------------------------------------------------

def getDevices(device_names: List[str]) -> List[List[Dict[str, Any]]]:
    """
    Load raw DB rows for each device name. Returns one list per device,
    preserving order. Empty inner lists mean that device has no mappings.

    Parameters:
        device_names (list[str]): e.g. ['Joypad 1', 'Keyboard', 'Commands']

    Returns:
        list[list[dict]]: outer list mirrors device_names; inner list is DB rows.
    """
    _require_init()
    result = []
    for name in device_names:
        rows = _cursor.execute(
            'SELECT * FROM GPIOnext WHERE device = ?', (name,)
        ).fetchall()
        result.append(rows)
    return result


def getDevice(device_name: str) -> List[Dict[str, Any]]:
    """
    Load all DB rows for a single device.

    Parameters:
        device_name (str): exact device name (LIKE match for partial names)

    Returns:
        list[dict]: rows for the device; empty list if none.
    """
    _require_init()
    return _cursor.execute(
        'SELECT * FROM GPIOnext WHERE device LIKE ?', (device_name,)
    ).fetchall()


def getDeviceRaw(device_name: str) -> List[Dict[str, Any]]:
    """
    Alias for getDevice; kept for backward compatibility with config_manager.py.

    Parameters:
        device_name (str): exact or LIKE-pattern device name

    Returns:
        list[dict]: raw DB rows
    """
    return getDevice(device_name)


def getAllRows() -> List[Dict[str, Any]]:
    """
    Return every row in the database. Used by import/export and the live pin view.

    Returns:
        list[dict]: all rows ordered by id.
    """
    _require_init()
    return _cursor.execute('SELECT * FROM GPIOnext ORDER BY id').fetchall()


# ---------------------------------------------------------------------------
# Write operations
# ---------------------------------------------------------------------------

def updateEntry(entry: dict) -> None:
    """
    Insert or replace a single row. The dict must contain all column keys:
    id, device, name, type, command, pins.
    Pass id=None to insert a new row (SQLite auto-increments).

    Parameters:
        entry (dict): keys: id, device, name, type, command, pins
    """
    _require_init()
    _cursor.execute(
        'INSERT OR REPLACE INTO GPIOnext (id, device, name, type, command, pins) '
        'VALUES (:id, :device, :name, :type, :command, :pins)',
        entry
    )
    _conn.commit()


def createDevice(rows: List[Tuple]) -> None:
    """
    Bulk-insert multiple rows for a new device.

    Parameters:
        rows (list[tuple]): each tuple is (device, name, type, command, pins)
    """
    _require_init()
    _cursor.executemany(
        'INSERT INTO GPIOnext (device, name, type, command, pins) VALUES (?,?,?,?,?)',
        rows
    )
    _conn.commit()


def deleteEntry(entry: dict) -> None:
    """
    Delete a single row by id.

    Parameters:
        entry (dict): must contain 'id' key
    """
    _require_init()
    _cursor.execute('DELETE FROM GPIOnext WHERE id = :id', entry)
    _conn.commit()


def deleteDevice(device_name: str) -> None:
    """
    Delete all rows for a device (used by 'Clear Device' menu option).

    Parameters:
        device_name (str): exact device name to delete
    """
    _require_init()
    _cursor.execute('DELETE FROM GPIOnext WHERE device = ?', (device_name,))
    _conn.commit()


# ---------------------------------------------------------------------------
# Import / Export (JSON)
# ---------------------------------------------------------------------------

def exportToJson() -> dict:
    """
    Return all configuration tables as a dict suitable for json.dumps.

    Returns:
        dict: containing 'GPIOnext', 'I2C_MCP23017', 'I2C_ADS1115', 'I2C_PCF8574' keys.
    """
    _require_init()
    return {
        'GPIOnext': _cursor.execute('SELECT * FROM GPIOnext ORDER BY id').fetchall(),
        'I2C_MCP23017': _cursor.execute('SELECT * FROM I2C_MCP23017 ORDER BY id').fetchall(),
        'I2C_ADS1115': _cursor.execute('SELECT * FROM I2C_ADS1115 ORDER BY id').fetchall(),
        'I2C_PCF8574': _cursor.execute('SELECT * FROM I2C_PCF8574 ORDER BY id').fetchall()
    }


def importFromJson(data: Union[Dict, List], replace: bool = True) -> None:
    """
    Import configuration from a JSON export. 
    Supports both the new dict format and the legacy list format.

    Parameters:
        data    (dict|list): rows from a previous exportToJson() call
        replace (bool): if True, wipes the database before importing
    """
    _require_init()
    
    if isinstance(data, list):
        # Legacy format: only GPIOnext table
        rows_gpionext = data
        rows_mcp = []
        rows_ads = []
        rows_pcf = []
    else:
        rows_gpionext = data.get('GPIOnext', [])
        rows_mcp = data.get('I2C_MCP23017', [])
        rows_ads = data.get('I2C_ADS1115', [])
        rows_pcf = data.get('I2C_PCF8574', [])

    if replace:
        _cursor.execute('DELETE FROM GPIOnext')
        _cursor.execute('DELETE FROM I2C_MCP23017')
        _cursor.execute('DELETE FROM I2C_ADS1115')
        _cursor.execute('DELETE FROM I2C_PCF8574')

    for row in rows_gpionext:
        # Strip 'id' so SQLite auto-assigns; preserves relative order
        entry = {k: v for k, v in row.items() if k != 'id'}
        _cursor.execute(
            'INSERT INTO GPIOnext (device, name, type, command, pins) '
            'VALUES (:device, :name, :type, :command, :pins)',
            entry
        )
    
    for row in rows_mcp:
        entry = {k: v for k, v in row.items() if k != 'id'}
        _cursor.execute(
            'INSERT INTO I2C_MCP23017 (bus, address, int_pin) '
            'VALUES (:bus, :address, :int_pin)',
            entry
        )

    for row in rows_ads:
        entry = {k: v for k, v in row.items() if k != 'id'}
        _cursor.execute(
            'INSERT INTO I2C_ADS1115 (bus, address) '
            'VALUES (:bus, :address)',
            entry
        )

    for row in rows_pcf:
        entry = {k: v for k, v in row.items() if k != 'id'}
        _cursor.execute(
            'INSERT INTO I2C_PCF8574 (bus, address, int_pin) '
            'VALUES (:bus, :address, :int_pin)',
            entry
        )

    _conn.commit()


# ---------------------------------------------------------------------------
# Config dict builder (used by gpionext.py → GpioCore.start())
# ---------------------------------------------------------------------------

def buildConfigDict(args) -> dict:
    """
    Build the config dict that gpionext.py passes to gpionext_core.GpioCore.start().
    Translates raw DB rows into the format expected by lib.rs parse_peripherals().

    Parameters:
        args: argparse Namespace with combo_delay, key_hold_delay, debounce,
              pulldown, pins, dev, debug attributes

    Returns:
        dict: config dict with 'peripherals', 'combo_delay', 'key_hold_delay',
              'debounce', 'pulldown', 'pins', 'skip_pins' keys
    """
    from config.constants import DEVICE_INDEX

    rows = getAllRows()
    peripherals = []

    for row in rows:
        device_name = row['device']
        device_index = DEVICE_INDEX.get(device_name, 5)

        # pins stored as '11' (single), '(11, 13)' (combo/tuple),
        # or virtual I2C identifiers such as 'i2c-0x20-A0'.
        pins = []
        for parsed_pin in parse_pins_value(row['pins']):
            vpin = pin_value_to_vpin(parsed_pin)
            if vpin is not None:
                pins.append(vpin)

        peripherals.append({
            'name':         row['name'],
            'device_index': device_index,
            'type':         row['type'],
            'command':      str(row['command']),
            'pins':         pins,
        })

    skip_pins = []
    try:
        from config.hat_detect import detect_audio_hat
        hat = detect_audio_hat()
        if hat:
            skip_pins = hat.get('reserved_pins', [])
    except ImportError:
        pass

    use_i2c = getattr(args, 'use_i2c', False)
    if use_i2c:
        # BOARD 3 & 5 are I2C pins. We must skip them in gpiocdev if using I2C
        if 3 not in skip_pins:
            skip_pins.append(3)
        if 5 not in skip_pins:
            skip_pins.append(5)

    # Load I2C configurations only if use_i2c flag is set
    if use_i2c:
        mcp_list = _cursor.execute('SELECT bus, address, int_pin FROM I2C_MCP23017').fetchall()
        ads_list = _cursor.execute('SELECT bus, address FROM I2C_ADS1115').fetchall()
        pcf_list = _cursor.execute('SELECT bus, address, int_pin FROM I2C_PCF8574').fetchall()
    else:
        mcp_list = []
        ads_list = []
        pcf_list = []

    return {
        'peripherals':     peripherals,
        'i2c_mcp23017':    mcp_list,
        'i2c_ads1115':     ads_list,
        'i2c_pcf8574':     pcf_list,
        'combo_delay':     int(getattr(args, 'combo_delay', 50)),
        'key_hold_delay':  int(getattr(args, 'key_hold_delay', 350)),
        'debounce':        int(getattr(args, 'debounce', 1)),
        'pulldown':        bool(getattr(args, 'pulldown', False)),
        'pins':            list(getattr(args, 'pins', [])),
        'skip_pins':       skip_pins,
        'use_i2c':         use_i2c,
    }

def _map_i2c_pin_string_to_vpin(s: str) -> int:
    """
    Map a string like 'i2c-0x20-A0' to its virtual BOARD pin number.
    MCP23017: 64 + (addr-0x20)*16 + (0 if A else 8) + bit
    ADS1115: 128 + (addr-0x48)*4 + channel
    PCF8574: 192 + (addr-0x20)*8 + pin
    """
    parts = s.split('-')
    addr = int(parts[1], 16)
    if parts[2].startswith('ch'):
        # ADS1115
        channel = int(parts[2][2:])
        return 128 + (addr - 0x48) * 4 + channel
    if parts[2].startswith('P'):
        # PCF8574
        pin = int(parts[2][1:])
        return 192 + (addr - 0x20) * 8 + pin

    # MCP23017
    port = 0 if parts[2][0] == 'A' else 8
    bit = int(parts[2][1:])
    return 64 + (addr - 0x20) * 16 + port + bit


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

def _require_init() -> None:
    """Raise RuntimeError if init() has not been called."""
    if _conn is None:
        raise RuntimeError('SQL.init() must be called before using any SQL functions')
