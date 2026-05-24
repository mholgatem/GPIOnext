"""
baudrate.py — Raspberry Pi I2C baudrate management.

Allows setting the i2c_arm_baudrate in /boot/firmware/config.txt or
/boot/config.txt to either 100,000 (Default) or 400,000 (Fast).

WARNING: This utility modifies system configuration files and requires
root privileges. It is intended for advanced users only.
"""
import os
import re
import sys
from typing import Optional

CONFIG_PATHS = [
    '/boot/firmware/config.txt',
    '/boot/config.txt'
]

BAUDRATE_DEFAULT = 100000
BAUDRATE_FAST = 400000

ADVANCED_WARNING = (
    "!!! WARNING: ADVANCED USERS ONLY !!!\n"
    "Modifying I2C baudrate can affect system stability and hardware compatibility.\n"
    "Only proceed if you understand the implications for your specific setup."
)

def get_current_baudrate() -> int:
    """
    Parse the system config file to find the currently configured baudrate.
    Returns BAUDRATE_DEFAULT if no explicit setting is found.
    """
    path = _resolve_config_path()
    if not path:
        return BAUDRATE_DEFAULT

    try:
        with open(path, 'r') as f:
            content = f.read()
            # Match dtparam=i2c_arm_baudrate=XXXX
            match = re.search(r'^\s*dtparam=i2c_arm_baudrate=(\d+)', content, re.MULTILINE)
            if match:
                return int(match.group(1))
    except (OSError, ValueError):
        pass

    return BAUDRATE_DEFAULT

def set_baudrate(rate: int) -> bool:
    """
    Update the system config file with the new baudrate.
    Requires root/sudo.

    Returns:
        bool: True if the file was modified, False otherwise.
    """
    if rate not in [BAUDRATE_DEFAULT, BAUDRATE_FAST]:
        raise ValueError(f"Unsupported baudrate: {rate}")

    path = _resolve_config_path()
    if not path:
        print(f"ERROR: Could not find system config file at {CONFIG_PATHS}")
        return False

    try:
        with open(path, 'r') as f:
            lines = f.readlines()

        # Check if already set
        current = get_current_baudrate()
        if current == rate:
            return False

        new_lines = []
        found = False
        pattern = re.compile(r'^\s*dtparam=i2c_arm_baudrate=')

        for line in lines:
            if pattern.match(line):
                new_lines.append(f"dtparam=i2c_arm_baudrate={rate}\n")
                found = True
            else:
                new_lines.append(line)

        if not found:
            # Append to the end if not found
            if new_lines and not new_lines[-1].endswith('\n'):
                new_lines[-1] += '\n'
            new_lines.append(f"dtparam=i2c_arm_baudrate={rate}\n")

        # Write back (will fail if not root)
        with open(path, 'w') as f:
            f.writelines(new_lines)

        return True

    except OSError as e:
        print(f"ERROR: Failed to update {path}: {e}")
        if e.errno == 13: # Permission denied
            print("Hint: Run this utility with sudo.")
        return False

def _resolve_config_path() -> Optional[str]:
    """Find the first existing config path."""
    for path in CONFIG_PATHS:
        if os.path.exists(path):
            return path
    return None

if __name__ == "__main__":
    # Simple CLI for testing/manual use
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} [get | 100000 | 400000]")
        sys.exit(1)

    cmd = sys.argv[1]
    if cmd == "get":
        print(get_current_baudrate())
    elif cmd in ["100000", "400000"]:
        print(ADVANCED_WARNING)
        confirm = input("Are you sure you want to continue? (y/N): ")
        if confirm.lower() == 'y':
            if set_baudrate(int(cmd)):
                print("Baudrate updated. A REBOOT IS REQUIRED for changes to take effect.")
            else:
                print("Baudrate already set or update failed.")
        else:
            print("Aborted.")
    else:
        print(f"Unknown command: {cmd}")
