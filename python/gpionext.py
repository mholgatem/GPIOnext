#!/usr/bin/env python3
"""
gpionext.py — Main daemon entry point.

Thin Python wrapper around the gpionext_core Rust extension:
  - Parse CLI arguments (same flags as reference for backward compat)
  - Detect audio HATs and warn about reserved pins
  - Load config from SQLite and pass it to GpioCore
  - Install signal handlers (SIGTERM → stop, SIGHUP → reload)
  - Sleep forever while Rust handles all GPIO events

Run as: /opt/gpionext/venv/bin/python3 -u gpionext.py [flags]
"""
import argparse
from typing import Optional
import os
import signal
import sys
import time
from datetime import datetime

# Ensure both the python/ package directory and install root are on sys.path
# so gpionext_core.so can be imported regardless of the current working dir.
_PYTHON_DIR = os.path.dirname(os.path.realpath(__file__))
_INSTALL_ROOT = os.path.dirname(_PYTHON_DIR)
sys.path.insert(0, _PYTHON_DIR)
sys.path.insert(0, _INSTALL_ROOT)

import config.SQL as SQL
from config.constants import AVAILABLE_PINS_STRING
from config.hat_detect import detect_audio_hat, format_hat_warning


def _import_error_message(exc: ImportError) -> str:
    """Build a detailed import failure message for gpionext_core."""
    expected_paths = (
        '/opt/gpionext/gpionext_core.so',
        '/opt/gpionext/gpionext_core-armv7l.so',
        '/opt/gpionext/gpionext_core-aarch64.so',
        '/opt/gpionext/gpionext_core-x86_64.so',
    )
    path_state_lines = [
        f'  - {path}: {"present" if os.path.exists(path) else "missing"}'
        for path in expected_paths
    ]

    header = 'ERROR: Failed to import gpionext_core.'
    details = f'ImportError details: {exc}'

    if os.path.exists('/opt/gpionext/gpionext_core.so'):
        guidance = (
            'The extension file exists but failed to load. This usually means binary/runtime '
            'incompatibility (glibc version, architecture mismatch, or missing linked symbols).\n'
            'If you see GLIBC_* errors, rebuild/release gpionext_core in an older baseline '
            'environment for this target OS.'
        )
    else:
        guidance = (
            'Core binary appears missing. Install or update the architecture-specific binary:\n'
            '  /opt/gpionext/setup.sh --update-core\n'
            'or download a release asset and symlink it as /opt/gpionext/gpionext_core.so'
        )

    return '\n'.join([
        header,
        details,
        'Checked core paths:',
        *path_state_lines,
        guidance,
    ])


try:
    import gpionext_core
except ImportError as exc:
    sys.exit(_import_error_message(exc))
    
# ---------------------------------------------------------------------------
# CLI arguments
# ---------------------------------------------------------------------------

parser = argparse.ArgumentParser(description='GPIOnext — GPIO to HID daemon')

parser.add_argument('--combo_delay',
                    metavar='50', default=50, type=int,
                    help='Combo window in milliseconds (default: 50)')

parser.add_argument('--key_hold_delay',
                    metavar='350', default=350, type=int,
                    help='Milliseconds before keyboard key starts repeating (default: 350)')

parser.add_argument('--pins',
                    metavar='3,5,7,11', type=str,
                    default=AVAILABLE_PINS_STRING,
                    help='Comma-delimited BOARD pin numbers to watch')

parser.add_argument('--debounce',
                    metavar='1', default=1, type=int,
                    help='Debounce time in milliseconds (default: 1)')

parser.add_argument('--pulldown',
                    dest='pulldown', default=False, action='store_true',
                    help='Use pulldown resistors instead of pullup')

parser.add_argument('--use_i2c',
                    dest='use_i2c', default=False, action='store_true',
                    help='Enable I2C hardware (MCP23017/ADS1115/PCF8574). Disables GPIO on pins 3 and 5.')

parser.add_argument('--dev',
                    dest='dev', default=False, action='store_true',
                    help='Write log output to stdout')

parser.add_argument('--debug',
                    dest='debug', default=False, action='store_true',
                    help='Write log output to /opt/gpionext/logFile.txt')


# ---------------------------------------------------------------------------
# Daemon class
# ---------------------------------------------------------------------------

class GPIOnext:
    """
    Handle startup, signal handling, and clean shutdown.
    After init completes, the process sleeps while Rust owns all GPIO I/O.
    """

    def __init__(self, args: argparse.Namespace) -> None:
        self.args = self._normalise_args(args)
        self._log_file = None
        self._core: Optional[gpionext_core.GpioCore] = None

        self._open_log()

        # Signal handlers
        for sig in (signal.SIGTERM, signal.SIGQUIT, signal.SIGINT):
            signal.signal(sig, self._shutdown)
        signal.signal(signal.SIGHUP, self._reload)

        SQL.init()
        self._start_core()
        self._main()

    # ---------------------------------------------------------------------------
    # Lifecycle
    # ---------------------------------------------------------------------------

    def _start_core(self) -> None:
        """Build config dict from DB and start the Rust GPIO core."""
        # Audio HAT detection — must happen before building config so that
        # skip_pins is populated before the Rust event loop opens GPIO lines
        hat = detect_audio_hat()
        if hat:
            warning = format_hat_warning(hat)
            self.log(warning)
            print(warning)

        config_dict = SQL.buildConfigDict(self.args)
        if self._i2c_configured(config_dict) and not getattr(gpionext_core, 'i2c_enabled', lambda: False)():
            self.log('WARNING: I2C chips are configured, but gpionext_core was built without I2C support; virtual I2C pins will remain inactive.')
        self.log(f'Starting GPIOnext core with {len(config_dict["peripherals"])} peripherals')

        self._core = gpionext_core.GpioCore()
        self._core.start(config_dict)
        self.log('GPIOcore started')

    def _reload(self, sig: int, frame) -> None:
        """SIGHUP handler: hot-reload config from SQLite without restarting."""
        self.log('Received SIGHUP — reloading configuration')
        if self._core:
            config_dict = SQL.buildConfigDict(self.args)
            if self._i2c_configured(config_dict) and not getattr(gpionext_core, 'i2c_enabled', lambda: False)():
                self.log('WARNING: I2C chips are configured, but gpionext_core was built without I2C support; virtual I2C pins will remain inactive.')
            self._core.reload(config_dict)
        self.log('Reload complete')

    def _shutdown(self, sig: int, frame) -> None:
        """SIGTERM / SIGINT / SIGQUIT handler: clean shutdown."""
        self.log(f'Received signal {sig} — shutting down')
        if self._core:
            self._core.stop()
        if self._log_file:
            self._log_file.close()
        print()
        sys.exit(0)

    def _main(self) -> None:
        """Sleep forever - all GPIO work is done in Rust threads."""
        try:
            while True:
                time.sleep(3)
        except KeyboardInterrupt:
            self._shutdown(signal.SIGINT, None)

    # ---------------------------------------------------------------------------
    # Helpers
    # ---------------------------------------------------------------------------


    @staticmethod
    def _i2c_configured(config_dict: dict) -> bool:
        """Return True when the runtime config contains any I2C chip rows."""
        return any(
            config_dict.get(key)
            for key in ('i2c_mcp23017', 'i2c_ads1115', 'i2c_pcf8574')
        )

    def _normalise_args(self, args: argparse.Namespace) -> argparse.Namespace:
        """
        Convert raw argparse values to the types used internally.
        Keeps combo_delay in milliseconds (Rust handles unit conversion).
        """
        args.pins = [int(x.strip()) for x in args.pins.split(',') if x.strip()]
        return args

    def _open_log(self) -> None:
        """Open the debug log file if --debug flag is set."""
        if self.args.debug:
            log_path = '/opt/gpionext/logFile.txt'
            try:
                self._log_file = open(log_path, 'w')
            except OSError as exc:
                print(f'WARNING: Cannot open log file {log_path}: {exc}')

    def log(self, msg: str) -> None:
        """
        Write a timestamped log message to file and/or stdout.

        Parameters:
            msg (str): message to log; written if --debug or --dev is set
        """
        if not (self.args.debug or self.args.dev):
            return
        timestamp = datetime.now().strftime('%Y-%m-%d %I:%M:%S%p')
        line = f'{timestamp} SYSTEM - {msg}\n'
        if self.args.debug and self._log_file:
            self._log_file.write(line)
            self._log_file.flush()
        if self.args.dev:
            print(line, end='')


if __name__ == '__main__':
    args = parser.parse_args()
    GPIOnext(args)
