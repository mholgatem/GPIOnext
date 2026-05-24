"""
live_pin_view.py — Real-time GPIO pin monitor widget for Textual.

Displays all configured BOARD pins in a Textual Static widget.
Each pin row shows:
  - Pin number
  - Current state (pressed ● or idle ○)
  - What the pin is mapped to (device + action name), or 'unmapped'

Actively-pressed pins are highlighted. The display refreshes every ~50ms.
"""
import sys
import os
from typing import List, Dict

from textual.widgets import Static
from textual.app import RenderResult
from textual.reactive import reactive

_UI_DIR = os.path.dirname(os.path.realpath(__file__))
_PYTHON_DIR = os.path.dirname(_UI_DIR)
_INSTALL_ROOT = os.path.dirname(_PYTHON_DIR)
sys.path.insert(0, _PYTHON_DIR)
sys.path.insert(0, _INSTALL_ROOT)
sys.path.insert(0, '/opt/gpionext')

import config.SQL as SQL

try:
    import gpionext_core
    _HAS_CORE = True
except ImportError:
    _HAS_CORE = False

# ---------------------------------------------------------------------------
# Pin label builder
# ---------------------------------------------------------------------------

def build_pin_labels(all_pins: List[int], db_rows: List[Dict]) -> Dict[int, str]:
    """
    Build a map from BOARD pin number → display label ("Joypad 1 → START").
    """
    labels: Dict[int, str] = {p: 'unmapped' for p in all_pins}

    for row in db_rows:
        pin_list = []
        for parsed_pin in SQL.parse_pins_value(row.get('pins', '')):
            vpin = SQL.pin_value_to_vpin(parsed_pin)
            if vpin is not None:
                pin_list.append(vpin)

        label = f"{row['device']} \u2192 {row['name']}"
        for pin in pin_list:
            if pin in labels:
                existing = labels[pin]
                if existing == 'unmapped':
                    labels[pin] = label
                else:
                    labels[pin] = existing + ' / ' + label

    return labels


def _display_pin_name(pin: int) -> str:
    """Return a clear display name for physical and virtual pins."""
    if pin >= 192:
        addr = 0x20 + (pin - 192) // 8
        pcf_pin = (pin - 192) % 8
        return f"PCF8574 0x{addr:02X} P{pcf_pin}"
    if pin >= 128:
        addr = 0x48 + (pin - 128) // 4
        channel = (pin - 128) % 4
        return f"ADS1115 0x{addr:02X} CH{channel}"
    if pin >= 64:
        addr = 0x20 + (pin - 64) // 16
        port = 'A' if ((pin - 64) % 16) < 8 else 'B'
        bit = (pin - 64) % 8
        return f"MCP23017 0x{addr:02X} {port}{bit}"
    return f"BOARD {pin}"

# ---------------------------------------------------------------------------
# Main view widget
# ---------------------------------------------------------------------------

class LivePinView(Static):
    """
    Textual widget for real-time GPIO pin monitoring.
    """

    DEFAULT_CSS = """
    LivePinView {
        width: 100%;
        height: 1fr;
        background: $background;
        color: $text;
        border: solid #2C363F;
        padding: 1 2;
        overflow-y: auto;
    }
    """

    bitmask = reactive(0)
    pin_labels = reactive({})

    def __init__(self, pins: List[int], db_rows: List[Dict], **kwargs) -> None:
        super().__init__(**kwargs)
        self.pins = sorted(pins)
        self.pin_labels = build_pin_labels(pins, db_rows)

    def on_mount(self) -> None:
        """Start the polling interval on mount."""
        self.set_interval(0.05, self.update_pins)

    def update_pins(self) -> None:
        """Poll the GPIO core for current pin states."""
        if _HAS_CORE:
            try:
                self.bitmask = gpionext_core.get_pin_states()
            except Exception:
                pass

    def update_labels(self, db_rows: List[Dict]) -> None:
        """Update the displayed mapping labels."""
        self.pin_labels = build_pin_labels(self.pins, db_rows)

    def _render_pin_row(self, pin: int) -> str:
        pressed = bool(self.bitmask & (1 << pin))
        label = self.pin_labels.get(pin, 'unmapped')
        is_mapped = label != 'unmapped'
        
        state_char = '●' if pressed else '○'
        state_color = "cyan" if pressed else "dim"
        
        pin_name = f"P{pin}"
        if pin < 64:
            board_name = f"BOARD{pin}"
        else:
            board_name = _display_pin_name(pin).split(' ')[-1] # Just the last part like "A0", "P6", "CH0"
        
        mapped_text = label if is_mapped else "--"
        
        # Determine row style
        if pressed:
            row_style = "#ffc300 bold"
        elif not is_mapped:
            row_style = "dim"
        else:
            row_style = None

        # Build the line with proper markup
        line_content = f"  {pin_name:<6} {board_name:<10} [{state_color}]{state_char:<7}[/] {mapped_text}"
        if row_style:
            return f"[{row_style}]{line_content}[/]"
        return line_content

    def render(self) -> RenderResult:
        """Render the pin monitor table."""
        lines = []
        lines.append("[bold cyan]GPIONEXT — LIVE PIN MONITOR[/]")
        lines.append("[dim]○ = OFF     [cyan]●[/] = ON[/]\n")
        lines.append(f"[bold]  {'PIN':<6} {'BOARD':<10} {'STATE':<7} {'MAPPED TO'}[/]")

        # Separate pins into groups
        board_pins = [p for p in self.pins if p < 64]
        i2c_pins = [p for p in self.pins if p >= 64]

        # Render Board Pins
        for pin in board_pins:
            lines.append(self._render_pin_row(pin))

        # Group and Render I2C Pins
        if i2c_pins:
            current_chip = None
            for pin in i2c_pins:
                chip_name = _display_pin_name(pin).rsplit(' ', 1)[0]
                if chip_name != current_chip:
                    current_chip = chip_name
                    lines.append(f"\n[bold cyan]{chip_name.upper()}[/]")
                lines.append(self._render_pin_row(pin))

        return "\n".join(lines)
