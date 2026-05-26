#!/usr/bin/env python3
"""
config_manager.py — Interactive GPIOnext configuration tool.

Migrated to Textual for a modern async TUI.
"""
import argparse
import json
import os
import re
import signal
import subprocess
import sys
import time
from typing import List, Dict, Optional, Any, Tuple, Set

from textual.app import App, ComposeResult
from textual.containers import Container, Horizontal, Vertical, Grid, ScrollableContainer
from textual.widgets import (
    Header, Footer, Static, Button, Label, TabbedContent, TabPane, 
    DataTable, Input, Checkbox, Select, Switch, SelectionList
)
from textual.widgets.selection_list import Selection
from textual.screen import ModalScreen, Screen
from textual import on, work
from textual.reactive import reactive

# Ensure both the python/ package directory and install root are on sys.path
_UI_DIR = os.path.dirname(os.path.realpath(__file__))
_PYTHON_DIR = os.path.dirname(_UI_DIR)
_INSTALL_ROOT = os.path.dirname(_PYTHON_DIR)
sys.path.insert(0, _PYTHON_DIR)
sys.path.insert(0, _INSTALL_ROOT)
sys.path.insert(0, '/opt/gpionext')

import config.SQL as SQL
from config.constants import AVAILABLE_PINS, AVAILABLE_PINS_STRING, DEVICE_LIST, BUTTON_LIST, KEY_LIST, COMMAND_PRESETS
from ui.live_pin_view import LivePinView
import config.baudrate as baudrate
from ui.hat_presets import get_preset_names, get_display_name, preset_to_db_rows

try:
    import gpionext_core
    _HAS_CORE = True
except ImportError:
    _HAS_CORE = False

_SERVICE_FILE = "/lib/systemd/system/gpionext.service"


def _pins_to_str(pins) -> str:
    """
    Convert a pins value (list of ints or comma-separated string) to a
    display string suitable for the Daemon Settings Input widget.

    Parameters:
        pins: list of int pin numbers, or a comma-separated string.

    Returns:
        Comma-separated string of sorted pin numbers, or the original string.
    """
    if isinstance(pins, str):
        return pins
    return ','.join(str(p) for p in sorted(pins))


# ---------------------------------------------------------------------------
# Modal Screens
# ---------------------------------------------------------------------------

class SafeDismissMixin:
    """Mixin that prevents double-dismiss (ScreenStackError on Textual 0.43.2)."""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._dismissed = False

    def _safe_dismiss(self, result) -> None:
        if self._dismissed:
            return
        self._dismissed = True
        self.dismiss(result)


class PinCaptureModal(SafeDismissMixin, ModalScreen[Optional[List[int]]]):
    """Modal screen for capturing GPIO pin input."""

    DEFAULT_CSS = """
    #dialog {
        width: 60;
        height: 12;
    }
    #label {
        width: 100%;
        content-align: center middle;
        height: 3;
        text-style: bold;
    }
    #status {
        width: 100%;
        content-align: center middle;
        color: $success;
        height: 3;
    }
    """

    def __init__(self, label: str, hold_seconds: float = 1.0, **kwargs):
        super().__init__(**kwargs)
        self.target_label = label
        self.hold_seconds = hold_seconds
        self.hold_start: Optional[float] = None
        self.last_bitmask: int = 0
        self.waiting_for_release: bool = True

    def compose(self) -> ComposeResult:
        with Container(id="dialog", classes="modal-dialog"):
            yield Label(f"Configuring: [bold]{self.target_label}[/]", id="label")
            yield Label("Please release all pins...", id="status")
            with Horizontal(id="buttons", classes="modal-buttons"):
                yield Button("\\[ - Cancel (Esc) ]", variant="error", id="cancel")

    def on_mount(self) -> None:
        if not _HAS_CORE:
            self._safe_dismiss(None)
            return
        self.set_interval(0.05, self.poll_pins)

    def poll_pins(self) -> None:
        try:
            bitmask = gpionext_core.get_pin_states()
        except Exception:
            return

        if self.waiting_for_release:
            if bitmask == 0:
                self.waiting_for_release = False
                self.query_one("#status").update("Hold pin(s) to continue...")
            return

        if bitmask == 0:
            self.hold_start = None
            self.last_bitmask = 0
            self.query_one("#status").update("Hold pin(s) to continue...")
            return

        if bitmask != self.last_bitmask:
            self.hold_start = time.time()
            self.last_bitmask = bitmask
            self.query_one("#status").update("[yellow]Holding...[/]")
            return

        if self.hold_start and (time.time() - self.hold_start) >= self.hold_seconds:
            pins = [bit for bit in range(256) if bitmask & (1 << bit)]
            self._safe_dismiss(pins)

    @on(Button.Pressed, "#cancel")
    def cancel_capture(self) -> None:
        self._safe_dismiss(None)

    def on_key(self, event) -> None:
        if event.key == "escape":
            self._safe_dismiss(None)


class ConfirmModal(ModalScreen[bool]):
    """Generic confirmation modal."""
    
    DEFAULT_CSS = """
    #confirm-dialog {
        width: 50;
        height: auto;
        min-height: 12;
    }
    #confirm-message {
        margin: 1 0;
        width: 100%;
        height: auto;
        content-align: center middle;
    }
    """

    def __init__(self, title: str, message: str, **kwargs):
        super().__init__(**kwargs)
        self.title_text = title
        self.message_text = message

    def compose(self) -> ComposeResult:
        with Container(id="confirm-dialog", classes="modal-dialog"):
            yield Label(f"[bold]{self.title_text}[/]", id="confirm-title")
            yield Label(self.message_text, id="confirm-message")
            with Horizontal(id="confirm-buttons", classes="modal-buttons"):
                yield Button("\\[ Yes ]", variant="primary", id="yes")
                yield Button("\\[ No ]", variant="error", id="no")

    @on(Button.Pressed, "#yes")
    def handle_yes(self) -> None:
        self.dismiss(True)

    @on(Button.Pressed, "#no")
    def handle_no(self) -> None:
        self.dismiss(False)

    def on_key(self, event) -> None:
        if event.key == "escape":
            self.dismiss(False)


class MultiSelectionModal(SafeDismissMixin, ModalScreen[Optional[List[Any]]]):
    """Modal screen for selecting multiple items from a list."""

    DEFAULT_CSS = """
    #selection-dialog {
        width: 60;
        height: 80%;
    }
    #selection-list {
        height: 1fr;
    }
    """

    def __init__(self, title: str, options: List[Tuple[str, Any]], defaults: Set[Any] = None, **kwargs):
        super().__init__(**kwargs)
        self.title_text = title
        self.options = options
        self.defaults = defaults or set()

    def compose(self) -> ComposeResult:
        with Vertical(id="selection-dialog", classes="modal-dialog"):
            yield Label(f"[bold]{self.title_text}[/]")
            yield SelectionList(
                *[Selection(label, value, value in self.defaults) for label, value in self.options],
                id="selection-list"
            )
            with Horizontal(id="selection-buttons", classes="modal-buttons"):
                yield Button("\\[ Confirm ✓ ]", variant="primary", id="confirm")
                yield Button("\\[ Cancel X ]", variant="error", id="cancel")

    @on(Button.Pressed, "#confirm")
    def handle_confirm(self) -> None:
        self.dismiss(self.query_one(SelectionList).selected)

    @on(Button.Pressed, "#cancel")
    def handle_cancel(self) -> None:
        self._safe_dismiss(None)

    def on_key(self, event) -> None:
        if event.key == "escape":
            self._safe_dismiss(None)


class SingleSelectionModal(SafeDismissMixin, ModalScreen[Optional[Any]]):
    """Modal screen for selecting a single item from a list."""

    DEFAULT_CSS = """
    #single-dialog {
        width: 50;
        height: 15;
    }
    Select {
        margin-top: 1;
    }
    """

    def __init__(self, title: str, options: List[Tuple[str, Any]], value: Any = None, **kwargs):
        super().__init__(**kwargs)
        self.title_text = title
        self.options = options
        self.initial_value = value if value is not None else (options[0][1] if options else None)

    def compose(self) -> ComposeResult:
        with Vertical(id="single-dialog", classes="modal-dialog"):
            yield Label(f"[bold]{self.title_text}[/]")
            yield Select(self.options, value=self.initial_value)
            with Horizontal(id="single-buttons", classes="modal-buttons"):
                yield Button("\\[ Confirm ✓ ]", variant="primary", id="confirm")
                yield Button("\\[ Cancel X ]", variant="error", id="cancel")

    @on(Button.Pressed, "#confirm")
    def handle_confirm(self) -> None:
        self.dismiss(self.query_one(Select).value)

    @on(Button.Pressed, "#cancel")
    def handle_cancel(self) -> None:
        self._safe_dismiss(None)

    def on_key(self, event) -> None:
        if event.key == "escape":
            self._safe_dismiss(None)


class CommandInputModal(SafeDismissMixin, ModalScreen[Optional[Tuple[str, int]]]):
    """Modal screen for configuring a command with an optional timeout."""

    DEFAULT_CSS = """
    #cmd-dialog {
        width: 60;
    }
    .cmd-row {
        margin-top: 1;
        height: auto;
    }
    """

    def __init__(self, presets: List[Tuple[str, str]], **kwargs):
        super().__init__(**kwargs)
        self.presets = presets

    def compose(self) -> ComposeResult:
        with Vertical(id="cmd-dialog", classes="modal-dialog"):
            yield Label("[bold]Configure Command[/]")
            yield Select([("Custom", "")] + self.presets, id="preset-select")
            yield Input(placeholder="Enter bash command...", id="cmd-input", classes="cmd-row")
            yield Label("Timeout in seconds (0 = no timeout):", classes="cmd-row")
            yield Input(value="0", type="integer", id="timeout-input")
            with Horizontal(id="cmd-buttons", classes="modal-buttons"):
                yield Button("\\[ Confirm ✓ ]", variant="primary", id="confirm")
                yield Button("\\[ Cancel ]", variant="error", id="cancel")

    @on(Select.Changed, "#preset-select")
    def handle_preset_change(self, event: Select.Changed) -> None:
        if event.value:
            self.query_one("#cmd-input", Input).value = str(event.value)

    @on(Button.Pressed, "#confirm")
    def handle_confirm(self) -> None:
        cmd = self.query_one("#cmd-input", Input).value.strip()
        if not cmd:
            return
        try:
            timeout = int(self.query_one("#timeout-input", Input).value)
        except ValueError:
            timeout = 0
        self.dismiss((cmd, timeout))

    @on(Button.Pressed, "#cancel")
    def handle_cancel(self) -> None:
        self._safe_dismiss(None)

    def on_key(self, event) -> None:
        if event.key == "escape":
            self._safe_dismiss(None)


class NextCommandModal(ModalScreen[Optional[str]]):
    """Modal to ask if the user wants to set another command."""

    DEFAULT_CSS = """
    #next-cmd-dialog {
        width: 60;
        height: 14;
    }
    #next-cmd-buttons Button {
        width: 100%;
    }
    """

    def compose(self) -> ComposeResult:
        with Vertical(id="next-cmd-dialog", classes="modal-dialog"):
            yield Label("[bold]Would you like to set another command?[/]")
            with Vertical(id="next-cmd-buttons", classes="modal-buttons"):
                yield Button("\\[ ✓ Yes, on the SAME button ]", variant="primary", id="btn-same")
                yield Button("\\[ ✓ Yes, on a NEW button ]", variant="primary", id="btn-new")
                yield Button("\\[ X No, I'm finished ]", variant="error", id="btn-no")

    @on(Button.Pressed, "#btn-same")
    def handle_same(self) -> None:
        self.dismiss("SAME_BUTTON")

    @on(Button.Pressed, "#btn-new")
    def handle_new(self) -> None:
        self.dismiss("NEW_BUTTON")

    @on(Button.Pressed, "#btn-no")
    def handle_no(self) -> None:
        self.dismiss("NO")

    def on_key(self, event) -> None:
        if event.key == "escape":
            self.dismiss(None)


class AddI2cModal(SafeDismissMixin, ModalScreen[Optional[Tuple[str, int, int, Optional[int]]]]):
    """Modal for adding an I2C chip."""

    DEFAULT_CSS = """
    #i2c-dialog {
        width: 60;
    }
    .i2c-row {
        margin-top: 1;
        height: auto;
    }
    """

    def compose(self) -> ComposeResult:
        with Vertical(id="i2c-dialog", classes="modal-dialog"):
            yield Label("[bold]Add I2C Chip[/]")
            
            yield Label("Chip Type:", classes="i2c-row")
            yield Select([("MCP23017", "MCP23017"), ("ADS1115", "ADS1115"), ("PCF8574", "PCF8574")], value="MCP23017", id="i2c-type")
            
            yield Label("Bus:", classes="i2c-row")
            yield Select([("1", 1), ("0", 0)], value=1, id="i2c-bus")
            
            yield Label("Address (Hex):", classes="i2c-row")
            yield Input(value="20", id="i2c-address")
            
            yield Label("Interrupt Pin (Leave blank if None/ADS1115):", classes="i2c-row")
            yield Input(placeholder="e.g. 17", id="i2c-int")
            
            with Horizontal(id="i2c-buttons", classes="modal-buttons"):
                yield Button("\\[ Confirm ✓ ]", variant="primary", id="confirm")
                yield Button("\\[ Cancel X ]", variant="error", id="cancel")

    @on(Button.Pressed, "#confirm")
    def handle_confirm(self) -> None:
        chip_type = self.query_one("#i2c-type", Select).value
        bus = self.query_one("#i2c-bus", Select).value
        
        try:
            address = int(self.query_one("#i2c-address", Input).value, 16)
        except ValueError:
            self.app.notify("Invalid address format. Use hex (e.g. 20, 48)", severity="error")
            return
            
        int_str = self.query_one("#i2c-int", Input).value.strip()
        int_pin = None
        if int_str and chip_type != "ADS1115":
            try:
                int_pin = int(int_str)
            except ValueError:
                self.app.notify("Interrupt pin must be an integer", severity="error")
                return

        self.dismiss((chip_type, bus, address, int_pin))

    @on(Button.Pressed, "#cancel")
    def handle_cancel(self) -> None:
        self._safe_dismiss(None)

    def on_key(self, event) -> None:
        if event.key == "escape":
            self._safe_dismiss(None)


class InputModal(SafeDismissMixin, ModalScreen[Optional[str]]):
    """Modal screen for getting text input."""

    DEFAULT_CSS = """
    #input-dialog {
        width: 60;
        height: auto;
        min-height: 12;
    }
    .input-row {
        margin-top: 1;
        height: auto;
    }
    """

    def __init__(self, title: str, placeholder: str = "", default_value: str = "", **kwargs):
        super().__init__(**kwargs)
        self.title_text = title
        self.placeholder_text = placeholder
        self.default_value = default_value

    def compose(self) -> ComposeResult:
        with Vertical(id="input-dialog", classes="modal-dialog"):
            yield Label(f"[bold]{self.title_text}[/]")
            yield Input(value=self.default_value, placeholder=self.placeholder_text, id="text-input", classes="input-row")
            with Horizontal(id="input-buttons", classes="modal-buttons"):
                yield Button("\\[ Confirm ✓ ]", variant="primary", id="confirm")
                yield Button("\\[ Cancel X ]", variant="error", id="cancel")

    @on(Button.Pressed, "#confirm")
    def handle_confirm(self) -> None:
        self.dismiss(self.query_one("#text-input", Input).value)

    @on(Button.Pressed, "#cancel")
    def handle_cancel(self) -> None:
        self._safe_dismiss(None)

    def on_key(self, event) -> None:
        if event.key == "escape":
            self._safe_dismiss(None)


# ---------------------------------------------------------------------------
# Splash Screen
# ---------------------------------------------------------------------------

class SplashScreen(Screen):
    """Full-screen splash displayed at startup; auto-dismisses after 2.5 s or on keypress."""

    DEFAULT_CSS = """
    SplashScreen {
        align: center middle;
        background: #151B23;
    }
    #splash-content {
        width: auto;
        height: auto;
        content-align: center middle;
        text-align: left;
        padding: 2 4;
        border: double #00D2D3;
    }
    """

    _ART = (
        "[#00D2D3 bold]"
        "[#00D2D3 bold] ██████╗ ██████╗ ██╗  ██████╗     [/]\n"
        "[#13BFD7 bold]██╔════╝ ██╔══██╗██║ ██╔═══██╗    [/]\n"
        "[#26ACDB bold]██║ ███╗ ██████╔╝██║ ██║   ██║    [/]\n"
        "[#3999DF bold]██║  ██║ ██╔═══╝ ██║ ██║   ██║    [/]\n"
        "[#4C86E3 bold]╚██████║ ██║     ██║ ╚██████╔╝    [/]\n"
        "[#5F73E7 bold] ╚═════╝ ╚═╝     ╚═╝  ╚═════╝     [/]\n"
        "[#00D2D3 bold]                                  [/]\n"
        "[#715FEB bold]███╗  ██╗███████╗██╗  ██╗████████╗[/]\n" 
        "[#844CEF bold]████╗ ██║██╔════╝╚██╗██╔╝╚══██╔══╝[/]\n" 
        "[#9739F3 bold]██╔██╗██║█████╗   ╚███╔╝    ██║   [/]\n"
        "[#AA26F7 bold]██║╚████║██╔══╝   ██╔██╗    ██║   [/]\n"
        "[#BD13FB bold]██║ ╚███║███████╗██╔╝ ██╗   ██║   [/]\n"
        "[#D000ff bold]╚═╝  ╚══╝╚══════╝╚═╝  ╚═╝   ╚═╝   [/]\n"
        "\n\n"
        "[dim]GPIO Peripheral Manager for Raspberry Pi[/]\n\n"
        "[dim italic]Press any key to continue...[/]"
    )

    def compose(self) -> ComposeResult:
        yield Static(self._ART, id="splash-content")

    def on_mount(self) -> None:
        self._dismissed = False
        self.set_timer(5, self._do_dismiss)

    def on_key(self, event) -> None:
        self._do_dismiss()

    def _do_dismiss(self) -> None:
        if self._dismissed:
            return
        self._dismissed = True
        self.app.pop_screen()


# ---------------------------------------------------------------------------
# Main Application
# ---------------------------------------------------------------------------

class ConfigurationApp(App):
    """GPIOnext Configuration Manager Textual App."""

    CSS = """
    $background: #151B23;
    $surface: #1E252C;
    $accent: #00D2D3;
    $success: #00E676;
    $error: #FF5252;
    $warning: #ffc300;
    $border: #2C363F;
    $dimborder: #222a35;
    $text: #A9B1B8;
    
    Screen {
        background: $background;
        color: $text;
    }

    #custom-header {
        height: 3;
        background: $surface;
        border-bottom: solid $border;
        padding: 0 2;
        align: left middle;
    }

    #header-title {
        width: 1fr;
        text-style: bold;
        color: $text;
    }

    #main-container {
        layout: vertical;
        height: 1fr;
    }

    #panels-container {
        layout: horizontal;
        height: 1fr;
    }

    #left-panel {
        width: 60%;
        border-right: solid $border;
    }
    #right-panel {
        width: 40%;
        height: 1fr;
    }
    #pin-scroll {
        height: 1fr;
        overflow-y: auto;
        overflow-x: auto;
    }
    #settings-grid {
        grid-size: 2;
        grid-columns: 1fr 1fr;
        grid-rows: auto;
        margin: 1 0;
    }
    .settings-label {
        padding: 1 0;
        content-align: left middle;
    }
    .settings-input {
        width: 100%;
    }

    TabbedContent {
        height: 100%;
    }

    Tabs {
        background: $background;
    }

    Tab.-active {
        background: $background;
        color: $warning;
        text-style: bold;
    }

    Tab:hover {
        text-style: reverse;
    }

    TabbedContent #--content {
        background: $background;
    }

    TabPane {
        padding: 1 2;
    }
    
    #tab-devices {
        background: $background;
        color: $accent;
    }
    #tab-mappings {
        background: $background;
        color: $accent;
    }
    #tab-settings {
        background: $background;
        color: $accent;
    }
    #tab-presets {
        background: $background;
        color: $accent;
    }
    
    .btn-global {
        background: $background;
    }
    
    .btn-global:hover, .btn-global:focus {
        background: transparent !important;
        text-style: reverse;
    }

    .device-button {
        margin: 1 0;
        width: 100%;
        background: $background;
    }

    #btn-dev-joypad1 { color: #48c2d4; background: $background; border: round #48c2d4; }
    #btn-dev-joypad2 { color: #66a0d9; background: $background; border: round #66a0d9; }
    #btn-dev-joypad3 { color: #857edf; background: $background; border: round #857edf; }
    #btn-dev-joypad4 { color: #a35be4; background: $background; border: round #a35be4; }
    #btn-dev-keyboard { color: #c239ea; background: $background; border: round #c239ea; }
    #btn-dev-commands { color: #e017ef; background: $background; border: round #e017ef; }

    #mappings-table {
        height: 1fr;
        border: round $border;
        background: $surface;
    }

    #mappings-table > .datatable--cursor {
        background: $accent 30%;
        color: $accent;
        text-style: bold;
    }

    .settings-row {
        height: auto;
        padding: 1 0;
        align: left middle;
    }

    .footer-buttons {
        height: 5;
        margin-top: 1;
        align: center middle;
    }

    .footer-buttons Button {
        margin: 0 1;
        border: round $border;
    }

    #btn-clear-mapping {
        color: $warning;
        border: round $warning;
    }

    #btn-clear-device, #btn-remove-i2c {
        color: $error;
        border: round $error;
    }
    
    #switch-i2c {
        border: round $success;
    }
        

    #btn-add-i2c {
        color: $success;
        border: round $success;
    }
    
    #btn-apply-preset {
        color: $success;
        border: round $success;
    }

    #btn-save-settings {
        color: $success;
        border: round $success;
    }

    #btn-pins-default {
        color: $warning;
        border: round $warning;
        background: $background;
    }

    #btn-export {
        color: $warning;
        border: round $warning;
        background: $background;
    }
    
    #btn-import {
        color: $warning;
        border: round $warning;
        background: $background;
    }

    #custom-footer {
        height: 3;
        background: $surface;
        border-top: solid $border;
        align: center middle;
        padding: 0 2;
    }

    .footer-key {
        color: $accent;
        text-style: bold;
        margin: 0 1;
        border: round $border;
        content-align: center middle;
        height: 3;
        min-width: 5;
    }

    .footer-label {
        margin-right: 2;
        height: 3;
        content-align: left middle;
    }

    /* Common Modal Styles */
    ModalScreen {
        align: center middle;
    }

    .modal-dialog {
        padding: 1 2;
        border: round $border;
        background: $surface;
        height: 80%;
    }

    .modal-buttons {
        margin-top: 1;
        content-align: center middle;
    }

    .modal-buttons Button {
        margin: 0 1;
    }

    Button.-primary {
        color: $success;
        border: round $success;
        background: transparent !important;
    }
    Button.-primary:hover, Button.-primary:focus {
        text-style: reverse;
        background: transparent !important;
    }

    Button.-error {
        color: $error;
        border: round $error;
        background: transparent !important;
    }
    Button.-error:hover, Button.-error:focus {
        text-style: reverse;
        background: transparent !important;
    }

    Input, Select, SelectionList {
        border: round $border;
        background: $background;
    }
    """

    BINDINGS = [
        ("q", "quit", "Quit"),
        ("escape", "go_back", "Back"),
    ]

    def __init__(self, args: argparse.Namespace):
        super().__init__()
        self.args = self._normalise_args(args)
        self._core: Optional[gpionext_core.GpioCore] = None
        SQL.init()
        
        # Auto-enable I2C if chips exist in the database and the flag wasn't explicitly passed
        if not getattr(self.args, 'use_i2c', False):
            mcp = SQL._cursor.execute('SELECT COUNT(*) FROM I2C_MCP23017').fetchone()['COUNT(*)']
            ads = SQL._cursor.execute('SELECT COUNT(*) FROM I2C_ADS1115').fetchone()['COUNT(*)']
            pcf = SQL._cursor.execute('SELECT COUNT(*) FROM I2C_PCF8574').fetchone()['COUNT(*)']
            if (mcp + ads + pcf) > 0:
                self.args.use_i2c = True
                
        # If I2C is enabled, ensure pins are restored to ALT0 so the hardware mux is correct
        if getattr(self.args, 'use_i2c', False):
            self._restore_i2c_pins()

    def action_go_back(self) -> None:
        """Move to the Devices tab when Escape is pressed."""
        try:
            tabs = self.query_one(TabbedContent)
            tabs.active = "tab-devices"
        except Exception:
            pass

    def _restore_i2c_pins(self) -> None:
        """Restores BOARD pins 3 and 5 (GPIO 2 and 3) to their ALT0 I2C function."""
        try:
            # Try pinctrl (Pi 4/5) then fallback to raspi-gpio (Pi 3)
            subprocess.call('pinctrl set 2 a0 && pinctrl set 3 a0', shell=True, stderr=subprocess.DEVNULL)
            subprocess.call('raspi-gpio set 2 a0 && raspi-gpio set 3 a0', shell=True, stderr=subprocess.DEVNULL)
        except Exception:
            pass

    def _normalise_args(self, args: argparse.Namespace) -> argparse.Namespace:
        if isinstance(args.pins, str):
            args.pins = [int(x.strip()) for x in args.pins.split(',') if x.strip()]
        return args

    def on_mount(self) -> None:
        self.push_screen(SplashScreen())
        self.refresh_mappings_table()
        self.refresh_i2c_table()
        self._stop_daemon()
        self._start_gpio_monitor()

    def _stop_daemon(self) -> None:
        subprocess.call(('systemctl', 'stop', 'gpionext'))

    def _start_gpio_monitor(self) -> None:
        if not _HAS_CORE:
            return
        self._core = gpionext_core.GpioCore()
        try:
            config_dict = SQL.buildConfigDict(self.args)
            self._core.start_monitor(config_dict)
        except RuntimeError:
            self._core = None

    def compose(self) -> ComposeResult:
        with Horizontal(id="custom-header"):
            yield Label("GPIOnext TERMINAL CONFIG v2.1.0", id="header-title")
        with Container(id="main-container"):
            with Horizontal(id="panels-container"):
                with Vertical(id="left-panel"):
                    with TabbedContent():
                        with TabPane("\\[ Devices ]", id="tab-devices"):
                            yield Label("[bold]Select a device to configure:[/]")
                            for device in DEVICE_LIST:
                                yield Button(device, id=f"btn-dev-{device.lower().replace(' ', '')}", classes="device-button btn-global")
                        
                        with TabPane("\\[ Mappings ]", id="tab-mappings"):
                            yield DataTable(id="mappings-table", cursor_type="row")
                            with Horizontal(classes="footer-buttons"):
                                yield Button("\\[ - Clear Selected Mapping ]", id="btn-clear-mapping", classes="btn-global")
                                yield Button("\\[ - Clear Device ]", id="btn-clear-device", classes="btn-global")
                        
                        with TabPane("\\[ I2C & Settings ] ", id="tab-settings"):
                            with Vertical():
                                with Horizontal(classes="settings-row"):
                                    yield Label("Enable I2C Hardware  ")
                                    yield Switch(value=getattr(self.args, 'use_i2c', False), id="switch-i2c")
                                
                                yield Label("Baudrate:")
                                yield Select(
                                    [("100kHz (Default)", 100000), ("400kHz (Fast)", 400000)], 
                                    value=baudrate.get_current_baudrate(), 
                                    id="select-baudrate"
                                )
                                
                                yield Label("\n[bold]Configured I2C Chips:[/]")
                                yield DataTable(id="i2c-table", cursor_type="row")
                                with Horizontal(classes="footer-buttons"):
                                    yield Button("\\[ + Add Chip ]", id="btn-add-i2c", classes="btn-global")
                                    yield Button("\\[ - Remove Selected Chip ]", id="btn-remove-i2c", classes="btn-global")
                        
                        with TabPane("\\[ Presets & Config ]", id="tab-presets"):
                            yield Label("[bold]HAT Presets:[/]")
                            yield Select([(get_display_name(p), p) for p in get_preset_names()], id="select-preset")
                            yield Button("\\[ + Apply Preset ]", id="btn-apply-preset", classes="btn-global")

                            yield Label("\n[bold]Configuration Management:[/]")
                            with Horizontal(classes="footer-buttons"):
                                yield Button("\\[ Export JSON → ]", id="btn-export", classes="btn-global")
                                yield Button("\\[ → Import JSON ]", id="btn-import", classes="btn-global")

                            yield Label("\n[bold]Daemon Settings:[/]")
                            with Grid(id="settings-grid"):
                                yield Label("combo_delay (ms):", classes="settings-label")
                                _w = Input(str(getattr(self.args, 'combo_delay', 50)),
                                           id="input-combo-delay", classes="settings-input",
                                           placeholder="50")
                                _w.tooltip = "Window (ms) for multi-button combos before input is processed (default: 50)"
                                yield _w
                                yield Label("key_hold_delay (ms):", classes="settings-label")
                                _w = Input(str(getattr(self.args, 'key_hold_delay', 350)),
                                           id="input-key-hold-delay", classes="settings-input",
                                           placeholder="350")
                                _w.tooltip = "Milliseconds before a held keyboard key begins repeating (default: 350)"
                                yield _w
                                yield Label("debounce (ms):", classes="settings-label")
                                _w = Input(str(getattr(self.args, 'debounce', 1)),
                                           id="input-debounce", classes="settings-input",
                                           placeholder="1")
                                _w.tooltip = "Ignore repeated GPIO signals within this window after a state change (default: 1)"
                                yield _w
                                yield Label("pins:", classes="settings-label")
                                _w = Input(_pins_to_str(self.args.pins),
                                           id="input-pins", classes="settings-input",
                                           placeholder="default")
                                _w.tooltip = "Comma-separated BOARD pin numbers to monitor. Leave blank or 'default' to use all available pins."
                                yield _w
                                yield Label("pulldown:", classes="settings-label")
                                _w = Switch(getattr(self.args, 'pulldown', False), id="switch-pulldown")
                                _w.tooltip = "Enable internal pull-down resistors on GPIO input pins"
                                yield _w
                                yield Label("dev mode:", classes="settings-label")
                                _w = Switch(getattr(self.args, 'dev', False), id="switch-dev")
                                _w.tooltip = "Log daemon output to journald — enables verbose output for 'gpionext journal'"
                                yield _w
                                yield Label("debug mode:", classes="settings-label")
                                _w = Switch(getattr(self.args, 'debug', False), id="switch-debug")
                                _w.tooltip = "Write detailed debug output to /opt/gpionext/logFile.txt"
                                yield _w
                            with Horizontal(classes="footer-buttons"):
                                yield Button("\\[ + Save Settings ]", id="btn-save-settings",
                                             classes="btn-global")
                                yield Button("\\[ Set pins to default ]", id="btn-pins-default",
                                             classes="btn-global")
                
                with Vertical(id="right-panel"):
                    db_rows = SQL.getAllRows()
                    pins_to_show = self._get_pins_to_show()
                    with ScrollableContainer(id="pin-scroll"):
                        yield LivePinView(pins_to_show, db_rows, id="live-monitor")

            with Horizontal(id="custom-footer"):
                yield Label("Q", classes="footer-key")
                yield Label("Quit", classes="footer-label")
                yield Label("Tab/←/↑/→/↓", classes="footer-key")
                yield Label("Navigate", classes="footer-label")
                yield Label("↵/\\[Space]", classes="footer-key")
                yield Label("Select/Toggle", classes="footer-label")
                yield Label("Esc", classes="footer-key")
                yield Label("Back", classes="footer-label")

    def _get_pins_to_show(self) -> List[int]:
        pins = list(self.args.pins)
        if getattr(self.args, 'use_i2c', False):
            # MCP23017
            mcp_chips = SQL._cursor.execute('SELECT address FROM I2C_MCP23017').fetchall()
            for mcp in mcp_chips:
                addr = mcp['address']
                base_vpin = 64 + (addr - 0x20) * 16
                pins.extend(range(base_vpin, base_vpin + 16))
            # ADS1115
            ads_chips = SQL._cursor.execute('SELECT address FROM I2C_ADS1115').fetchall()
            for ads in ads_chips:
                addr = ads['address']
                base_vpin = 128 + (addr - 0x48) * 4
                pins.extend(range(base_vpin, base_vpin + 4))
            # PCF8574
            pcf_chips = SQL._cursor.execute('SELECT address FROM I2C_PCF8574').fetchall()
            for pcf in pcf_chips:
                addr = pcf['address']
                base_vpin = 192 + (addr - 0x20) * 8
                pins.extend(range(base_vpin, base_vpin + 8))
        return pins

    @on(TabbedContent.TabActivated)
    def handle_tab_change(self, event: TabbedContent.TabActivated) -> None:
        if event.tab.id == "tab-mappings":
            self.refresh_mappings_table()
        elif event.tab.id == "tab-settings":
            self.refresh_i2c_table()

    def refresh_mappings_table(self) -> None:
        try:
            table = self.query_one("#mappings-table", DataTable)
            table.clear(columns=True)
            table.add_columns("ID", "Device", "Name", "Pins")
            rows = SQL.getAllRows()
            for row in rows:
                table.add_row(
                    str(row['id']),
                    row['device'], 
                    row['name'], 
                    SQL.format_pins_value(row['pins'])
                )
        except Exception:
            pass

    def refresh_i2c_table(self) -> None:
        try:
            table = self.query_one("#i2c-table", DataTable)
            table.clear(columns=True)
            table.add_columns("Type", "Bus", "Address", "Int Pin")
            
            mcp = SQL._cursor.execute('SELECT * FROM I2C_MCP23017').fetchall()
            for r in mcp: table.add_row("MCP23017", r['bus'], f"0x{r['address']:02X}", r['int_pin'] if r['int_pin'] is not None else "None", key=f"mcp_{r['id']}")
            
            ads = SQL._cursor.execute('SELECT * FROM I2C_ADS1115').fetchall()
            for r in ads: table.add_row("ADS1115", r['bus'], f"0x{r['address']:02X}", "N/A", key=f"ads_{r['id']}")
            
            pcf = SQL._cursor.execute('SELECT * FROM I2C_PCF8574').fetchall()
            for r in pcf: table.add_row("PCF8574", r['bus'], f"0x{r['address']:02X}", r['int_pin'] if r['int_pin'] is not None else "None", key=f"pcf_{r['id']}")
        except Exception as e:
            self.notify(f"I2C Table Error: {e}", severity="error")

    @on(Switch.Changed, "#switch-i2c")
    def handle_i2c_switch(self, event: Switch.Changed) -> None:
        self.args.use_i2c = event.value
        
        # Update systemd service file
        try:
            if os.path.exists(_SERVICE_FILE):
                with open(_SERVICE_FILE, 'r') as f:
                    lines = f.readlines()
                with open(_SERVICE_FILE, 'w') as f:
                    for line in lines:
                        if line.startswith('ExecStart='):
                            line = line.replace(' --use_i2c', '')
                            if event.value:
                                line = line.strip() + ' --use_i2c\n'
                        f.write(line)
                subprocess.call(['systemctl', 'daemon-reload'])
        except Exception as e:
            self.notify(f"Failed to update service file: {e}", severity="error")

        if event.value:
            self._restore_i2c_pins()

        # Restart the monitor
        if self._core:
            self._core.stop()
        self._start_gpio_monitor()
        # Update live view pins
        pins_to_show = self._get_pins_to_show()
        monitor = self.query_one("#live-monitor", LivePinView)
        monitor.pins = sorted(pins_to_show)
        monitor.update_labels(SQL.getAllRows())
        self.notify(f"I2C Hardware {'enabled' if event.value else 'disabled'}.")

    @on(Button.Pressed, "#btn-add-i2c")
    @work
    async def handle_add_i2c(self) -> None:
        result = await self.push_screen(AddI2cModal(), wait_for_dismiss=True)
        if result:
            chip_type, bus, address, int_pin = result
            table_name = f"I2C_{chip_type}"
            try:
                if chip_type == "ADS1115":
                    SQL._cursor.execute(f"INSERT INTO {table_name} (bus, address) VALUES (?, ?)", (bus, address))
                else:
                    SQL._cursor.execute(f"INSERT INTO {table_name} (bus, address, int_pin) VALUES (?, ?, ?)", (bus, address, int_pin))
                SQL._conn.commit()
                self.refresh_i2c_table()
                
                # Restart monitor to pick up new chip
                if self._core:
                    self._core.stop()
                self._start_gpio_monitor()
                # Update live view pins
                monitor = self.query_one("#live-monitor", LivePinView)
                monitor.pins = self._get_pins_to_show()
                monitor.update_labels(SQL.getAllRows())
                self.notify(f"Added {chip_type} at 0x{address:02X}")
            except Exception as e:
                self.notify(f"Database error: {e}", severity="error")

    @on(Button.Pressed, "#btn-remove-i2c")
    @work
    async def handle_remove_i2c(self) -> None:
        table = self.query_one("#i2c-table", DataTable)
        row_idx = table.cursor_row
        if row_idx is None or table.row_count == 0:
            self.notify("No I2C chip selected.", severity="warning")
            return
            
        try:
            row_key = table.coordinate_to_cell_key(table.cursor_coordinate).row_key.value
        except Exception:
            self.notify("Error resolving selected chip.", severity="error")
            return
            
        if not row_key:
            return
            
        prefix, db_id = row_key.split('_')
        table_map = {'mcp': 'I2C_MCP23017', 'ads': 'I2C_ADS1115', 'pcf': 'I2C_PCF8574'}
        db_table = table_map.get(prefix)
        
        if not db_table: return
        
        confirm = await self.push_screen(ConfirmModal("Remove Chip", f"Remove this {prefix.upper()} chip?"), wait_for_dismiss=True)
        if confirm:
            try:
                SQL._cursor.execute(f"DELETE FROM {db_table} WHERE id = ?", (int(db_id),))
                SQL._conn.commit()
                self.refresh_i2c_table()
                
                # Restart monitor
                if self._core:
                    self._core.stop()
                self._start_gpio_monitor()
                monitor = self.query_one("#live-monitor", LivePinView)
                monitor.pins = self._get_pins_to_show()
                monitor.update_labels(SQL.getAllRows())
                self.notify("I2C chip removed.")
            except Exception as e:
                self.notify(f"Database error: {e}", severity="error")

    @on(Button.Pressed, "#btn-clear-mapping")
    @work
    async def handle_clear_mapping(self, event: Button.Pressed) -> None:
        table = self.query_one("#mappings-table", DataTable)
        try:
            row_idx = table.cursor_row
            if row_idx is None:
                self.notify("No mapping selected to clear.", severity="warning")
                return
            row_data = table.get_row_at(row_idx)
            row_id = int(row_data[0])
            device_name = row_data[1]
            mapping_name = row_data[2]
            
            confirm = await self.push_screen(ConfirmModal(
                "Delete Mapping", 
                f"Are you sure you want to delete '{mapping_name}' from {device_name}?"
            ), wait_for_dismiss=True)
            if confirm:
                SQL.deleteEntry({'id': row_id})
                self.notify(f"Mapping '{mapping_name}' deleted.")
                self.refresh_mappings_table()
                self.query_one("#live-monitor").update_labels(SQL.getAllRows())
        except Exception as e:
            self.notify(f"Error: {e}", severity="error")

    @on(Button.Pressed, "#btn-clear-device")
    @work
    async def handle_clear_device(self, event: Button.Pressed) -> None:
        rows = SQL.getAllRows()
        # Find unique configured devices, excluding 'Commands'
        configured_devices = sorted(list(set(r['device'] for r in rows if r['device'] != 'Commands')))
        
        if not configured_devices:
            self.notify("No devices (other than Commands) are currently configured.", severity="warning")
            return
            
        options = [(dev, dev) for dev in configured_devices]
        
        device_to_clear = await self.push_screen(SingleSelectionModal(
            "Select Device to Clear",
            options
        ), wait_for_dismiss=True)
        
        if device_to_clear:
            confirm = await self.push_screen(ConfirmModal(
                "Clear Device", 
                f"Are you sure you want to delete ALL mappings for {device_to_clear}?"
            ), wait_for_dismiss=True)
            
            if confirm:
                SQL.deleteDevice(device_to_clear)
                self.notify(f"All mappings for {device_to_clear} deleted.")
                self.refresh_mappings_table()
                self.query_one("#live-monitor").update_labels(SQL.getAllRows())

    @on(Button.Pressed, "#btn-dev-joypad1")
    @on(Button.Pressed, "#btn-dev-joypad2")
    @on(Button.Pressed, "#btn-dev-joypad3")
    @on(Button.Pressed, "#btn-dev-joypad4")
    def handle_joypad_config(self, event: Button.Pressed) -> None:
        device_name = event.button.label
        self._run_joypad_wizard(str(device_name))

    @on(Button.Pressed, "#btn-dev-keyboard")
    def handle_keyboard_config(self, event: Button.Pressed) -> None:
        self._run_keyboard_wizard()

    @on(Button.Pressed, "#btn-dev-commands")
    def handle_command_config(self, event: Button.Pressed) -> None:
        self._run_command_wizard()

    @work
    async def _run_command_wizard(self) -> None:
        device_name = "Commands"
        entries = []
        current_pins_str = None
        cmd_count = 1

        self.notify("Starting Commands wizard")

        while True:
            # 1. Ask for command and timeout
            result = await self.push_screen(CommandInputModal(COMMAND_PRESETS), wait_for_dismiss=True)
            if result is None:
                self.notify("Configuration cancelled", severity="warning")
                return
            
            cmd_str, timeout = result
            
            # 2. Format with timeout if needed
            if timeout > 0:
                final_cmd = f"timeout {timeout} {cmd_str}"
            else:
                final_cmd = cmd_str

            # 3. Ask for pins (only if we aren't appending to the same button)
            if current_pins_str is None:
                pins = await self.push_screen(PinCaptureModal(f"Command {cmd_count}"), wait_for_dismiss=True)
                if pins is None:
                    self.notify("Configuration cancelled", severity="warning")
                    return
                current_pins_str = self._pins_to_str(pins)
                # Create a new entry
                entries.append([device_name, f"Command {cmd_count}", 'COMMAND', final_cmd, current_pins_str])
            else:
                # Append to the last entry
                last_entry = entries[-1]
                last_entry[3] = f"{last_entry[3]} ||| {final_cmd}"

            # 4. Ask what to do next
            next_step = await self.push_screen(NextCommandModal(), wait_for_dismiss=True)
            
            if next_step == "SAME_BUTTON":
                # Do not clear current_pins_str, do not increment cmd_count
                pass
            elif next_step == "NEW_BUTTON":
                current_pins_str = None
                cmd_count += 1
            else:
                # "NO" or None (Escape)
                break

        if entries:
            # Convert inner lists back to tuples for SQL
            final_entries = [tuple(e) for e in entries]
            SQL.deleteDevice(device_name)
            SQL.createDevice(final_entries)
            self.notify("Commands configuration saved!")
            self.refresh_mappings_table()
            self.query_one("#live-monitor").update_labels(SQL.getAllRows())

    @work
    async def _run_keyboard_wizard(self) -> None:
        device_name = "Keyboard"
        
        # Overwrite Check
        existing = [r for r in SQL.getAllRows() if r['device'] == device_name]
        if existing:
            confirm = await self.push_screen(ConfirmModal(
                "Overwrite Device", 
                f"{device_name} is already configured. This will overwrite your current configuration. Continue?"
            ), wait_for_dismiss=True)
            if not confirm:
                return

        # 1. Select keys to configure
        selected_keys = await self.push_screen(MultiSelectionModal(
            "Select Keys to Configure",
            KEY_LIST,
            defaults={key[1] for key in KEY_LIST[:4]} # Default to first 4 (arrows usually)
        ), wait_for_dismiss=True)
        if selected_keys is None: return

        self.notify("Starting Keyboard wizard")
        entries = []

        # 2. Iterate through selected keys
        for key_code in selected_keys:
            key_name = next(name for name, code in KEY_LIST if code == key_code)
            pins = await self.push_screen(PinCaptureModal(f"Keyboard: {key_name}"), wait_for_dismiss=True)
            if pins is None:
                self.notify("Configuration cancelled", severity="warning")
                return
            entries.append((device_name, key_name, 'KEY', str(key_code), self._pins_to_str(pins)))

        SQL.deleteDevice(device_name)
        SQL.createDevice(entries)
        self.notify("Keyboard configuration saved!")
        self.refresh_mappings_table()
        self.query_one("#live-monitor").update_labels(SQL.getAllRows())

    @work
    async def _run_joypad_wizard(self, device_name: str) -> None:
        # Overwrite Check
        existing = [r for r in SQL.getAllRows() if r['device'] == device_name]
        if existing:
            confirm = await self.push_screen(ConfirmModal(
                "Overwrite Device", 
                f"{device_name} is already configured. This will overwrite your current configuration. Continue?"
            ), wait_for_dismiss=True)
            if not confirm:
                return

        # 1. Ask for axis (DPad) count
        axis_count = await self.push_screen(SingleSelectionModal(
            "Configure Joypad", 
            [("No DPad/Joysticks", 0), ("1 DPad/Joystick", 1), ("2 DPad/Joysticks", 2), ("3 DPad/Joysticks", 3), ("4 DPad/Joysticks", 4)],
            value=1
        ), wait_for_dismiss=True)
        if axis_count is None: return

        # 2. Select buttons to configure
        selected_buttons = await self.push_screen(MultiSelectionModal(
            "Select Buttons to Configure",
            BUTTON_LIST,
            defaults={btn[1] for btn in BUTTON_LIST[:8]} # Default to first 8 common buttons
        ), wait_for_dismiss=True)
        if selected_buttons is None: return

        self.notify(f"Starting wizard for {device_name}")
        entries = []
        
        # Axes (Directionals)
        for i in range(1, axis_count + 1):
            for direction, (axis_code, value) in (
                ('UP',    (1, -255)),
                ('DOWN',  (1,  255)),
                ('LEFT',  (0, -255)),
                ('RIGHT', (0,  255)),
            ):
                label = f'DPAD {i} {direction}'
                pins = await self.push_screen(PinCaptureModal(f"{device_name}: {label}"), wait_for_dismiss=True)
                if pins is None:
                    self.notify("Configuration cancelled", severity="warning")
                    return
                entries.append((device_name, label, 'AXIS', f'(3, {axis_code}, {value})', self._pins_to_str(pins)))

        # Buttons
        for btn_code in selected_buttons:
            btn_name = next(name for name, code in BUTTON_LIST if code == btn_code)
            pins = await self.push_screen(PinCaptureModal(f"{device_name}: {btn_name}"), wait_for_dismiss=True)
            if pins is None:
                self.notify("Configuration cancelled", severity="warning")
                return
            entries.append((device_name, btn_name, 'BUTTON', str(btn_code), self._pins_to_str(pins)))

        SQL.deleteDevice(device_name)
        SQL.createDevice(entries)
        self.notify(f"{device_name} configuration saved!")
        self.refresh_mappings_table()
        self.query_one("#live-monitor").update_labels(SQL.getAllRows())

    @on(Button.Pressed, "#btn-apply-preset")
    @work
    async def handle_apply_preset(self) -> None:
        preset_key = self.query_one("#select-preset").value
        if not preset_key:
            return
            
        display_name = get_display_name(preset_key)
        confirm = await self.push_screen(ConfirmModal(
            "Load HAT Preset", 
            f"Apply '{display_name}'? This will overwrite existing mappings for affected devices."
        ), wait_for_dismiss=True)
        
        if confirm:
            rows = preset_to_db_rows(preset_key)
            devices_affected = {r[0] for r in rows}
            for device_name in devices_affected:
                SQL.deleteDevice(device_name)
            SQL.createDevice(rows)
            self.notify(f"Preset '{display_name}' applied.")
            self.refresh_mappings_table()
            self.query_one("#live-monitor").update_labels(SQL.getAllRows())

    def _get_user_home(self) -> str:
        """Get the actual user's home directory, even when running with sudo."""
        sudo_user = os.environ.get('SUDO_USER')
        if sudo_user:
            return os.path.expanduser(f"~{sudo_user}")
        return os.path.expanduser("~")

    @on(Button.Pressed, "#btn-export")
    @work
    async def handle_export(self) -> None:
        """Export the current configuration to a JSON file."""
        default_path = os.path.join(self._get_user_home(), "gpionext_config.json")
        filepath = await self.push_screen(InputModal(
            "Export Configuration",
            placeholder="Path to save JSON...",
            default_value=default_path
        ), wait_for_dismiss=True)
        if not filepath:
            return
            
        try:
            data = SQL.exportToJson()
            with open(filepath, 'w') as f:
                json.dump(data, f, indent=4)
            self.notify(f"Configuration exported to {filepath}")
        except Exception as e:
            self.notify(f"Export error: {e}", severity="error")

    @on(Button.Pressed, "#btn-import")
    @work
    async def handle_import(self) -> None:
        """Import configuration from a JSON file."""
        default_path = os.path.join(self._get_user_home(), "gpionext_config.json")
        filepath = await self.push_screen(InputModal(
            "Import Configuration",
            placeholder="Path to JSON file...",
            default_value=default_path
        ), wait_for_dismiss=True)
        if not filepath:
            return
            
        if not os.path.exists(filepath):
            self.notify(f"File not found: {filepath}", severity="error")
            return
            
        confirm = await self.push_screen(ConfirmModal(
            "Import Configuration", 
            "This will overwrite all current device mappings. Are you sure?"
        ), wait_for_dismiss=True)
        
        if confirm:
            try:
                with open(filepath, 'r') as f:
                    data = json.load(f)
                SQL.importFromJson(data, replace=True)
                
                # Refresh all UI components
                self.refresh_mappings_table()
                self.refresh_i2c_table()
                
                # Restart monitor to pick up new configuration (pins, I2C chips)
                if self._core:
                    self._core.stop()
                self._start_gpio_monitor()
                
                # Update live monitor pins
                monitor = self.query_one("#live-monitor", LivePinView)
                monitor.pins = self._get_pins_to_show()
                monitor.update_labels(SQL.getAllRows())
                
                self.notify(f"Configuration imported from {filepath}")
            except Exception as e:
                self.notify(f"Import error: {e}", severity="error")

    def _patch_service_flags(self, combo_delay: int, key_hold_delay: int,
                              debounce: int, pins_str: str,
                              pulldown: bool, dev: bool, debug: bool) -> None:
        """
        Patch the ExecStart line in the systemd service file with new flag values,
        then daemon-reload and restart the gpionext service.

        Parameters:
            combo_delay:     --combo_delay value in ms
            key_hold_delay:  --key_hold_delay value in ms
            debounce:        --debounce value in ms
            pins_str:        --pins value as a string; 'default' or '' omits the flag
            pulldown:        whether to include --pulldown flag
            dev:             whether to include --dev flag
            debug:           whether to include --debug flag
        """
        if not os.path.exists(_SERVICE_FILE):
            self.notify("Service file not found — running outside Pi?", severity="warning")
            return

        with open(_SERVICE_FILE, 'r') as f:
            content = f.read()

        def _strip_managed_flags(line: str) -> str:
            # Remove flags managed by this panel from the ExecStart line
            for flag in ('--combo_delay', '--key_hold_delay', '--debounce',
                         '--pulldown', '--dev', '--debug'):
                line = re.sub(r'\s+' + re.escape(flag) + r'(\s+\S+)?', '', line)
            line = re.sub(r'\s+--pins\s+\S+', '', line)
            return line.rstrip()

        new_lines = []
        for line in content.splitlines(keepends=True):
            if line.startswith('ExecStart='):
                line = _strip_managed_flags(line)
                line += f' --combo_delay {combo_delay}'
                line += f' --key_hold_delay {key_hold_delay}'
                line += f' --debounce {debounce}'
                if pins_str and pins_str.lower() != 'default':
                    line += f' --pins {pins_str.replace(" ", "")}'
                if pulldown:
                    line += ' --pulldown'
                if dev:
                    line += ' --dev'
                if debug:
                    line += ' --debug'
                line += '\n'
            new_lines.append(line)

        with open(_SERVICE_FILE, 'w') as f:
            f.writelines(new_lines)

        # daemon-reload registers the updated unit file; the daemon itself is
        # stopped while the config tool runs and will start with the new flags
        # when the config tool exits (see action_quit).
        subprocess.call(['systemctl', 'daemon-reload'])

    @on(Button.Pressed, "#btn-pins-default")
    def handle_pins_default(self) -> None:
        """Reset the pins Input to the full default BOARD pin list."""
        self.query_one("#input-pins", Input).value = AVAILABLE_PINS_STRING

    @on(Button.Pressed, "#btn-save-settings")
    @work
    async def handle_save_settings(self) -> None:
        """
        Read the Daemon Settings widgets, validate inputs, patch the service file,
        restart the daemon, and refresh the live pin monitor if the pins list changed.
        """
        combo_delay_str = self.query_one("#input-combo-delay", Input).value.strip()
        key_hold_str    = self.query_one("#input-key-hold-delay", Input).value.strip()
        debounce_str    = self.query_one("#input-debounce", Input).value.strip()
        pins_str        = self.query_one("#input-pins", Input).value.strip()
        pulldown        = self.query_one("#switch-pulldown", Switch).value
        dev             = self.query_one("#switch-dev", Switch).value
        debug           = self.query_one("#switch-debug", Switch).value

        try:
            combo_delay    = int(combo_delay_str) if combo_delay_str else 50
            key_hold_delay = int(key_hold_str)    if key_hold_str    else 350
            debounce       = int(debounce_str)    if debounce_str    else 1
        except ValueError:
            self.notify("combo_delay, key_hold_delay, and debounce must be whole numbers.",
                        severity="error")
            return

        old_pins_str = _pins_to_str(self.args.pins)
        pins_changed = pins_str != old_pins_str

        # Update in-memory args to keep the rest of the UI consistent
        self.args.combo_delay    = combo_delay
        self.args.key_hold_delay = key_hold_delay
        self.args.debounce       = debounce
        self.args.pulldown       = pulldown
        self.args.dev            = dev
        self.args.debug          = debug
        if pins_str and pins_str.lower() != 'default':
            self.args.pins = [int(x.strip()) for x in pins_str.split(',') if x.strip()]
        else:
            from config.constants import AVAILABLE_PINS
            self.args.pins = list(AVAILABLE_PINS)

        try:
            self._patch_service_flags(combo_delay, key_hold_delay, debounce,
                                       pins_str, pulldown, dev, debug)
            self.notify("Settings saved. New settings take effect when you exit the config tool.")
        except Exception as e:
            self.notify(f"Failed to save settings: {e}", severity="error")
            return

        if pins_changed:
            pins_to_show = self._get_pins_to_show()
            monitor = self.query_one("#live-monitor", LivePinView)
            monitor.pins = sorted(pins_to_show)
            monitor.update_labels(SQL.getAllRows())
            self.notify("Pin list updated — live monitor refreshed.", timeout=5)

    def _pins_to_str(self, pins: List[int]) -> str:
        out = []
        for p in pins:
            if p >= 192:
                addr = 0x20 + (p - 192) // 8
                pin = (p - 192) % 8
                out.append(f"i2c-0x{addr:02X}-P{pin}")
            elif p >= 128:
                addr = 0x48 + (p - 128) // 4
                ch = (p - 128) % 4
                out.append(f"i2c-0x{addr:02X}-ch{ch}")
            elif p >= 64:
                addr = 0x20 + (p - 64) // 16
                port = 'A' if ((p - 64) % 16) < 8 else 'B'
                bit = (p - 64) % 8
                out.append(f"i2c-0x{addr:02X}-{port}{bit}")
            else:
                out.append(str(p))
        
        if len(out) == 1:
            return out[0]
        return str(tuple(out))

    def action_quit(self) -> None:
        if self._core:
            self._core.stop()
        if getattr(self.args, 'use_i2c', False):
            self._restore_i2c_pins()
        subprocess.call(('systemctl', 'start', 'gpionext'))
        self.exit()

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def check_truecolor():
    """Check if True Color is enabled, and prompt to enable it in .bashrc if not."""
    if os.environ.get("COLORTERM") in ("truecolor", "24bit"):
        return

    sudo_user = os.environ.get('SUDO_USER')
    if sudo_user:
        home = os.path.expanduser(f"~{sudo_user}")
    else:
        home = os.path.expanduser("~")
    
    bashrc_path = os.path.join(home, ".bashrc")
    
    # Check if it's already in .bashrc but just not exported in current session
    if os.path.exists(bashrc_path):
        with open(bashrc_path, 'r') as f:
            content = f.read()
            if "COLORTERM=truecolor" in content:
                os.environ["COLORTERM"] = "truecolor"
                return

    # If we got here, it's not enabled.
    print("\nTrue Color is not enabled. Textual works best with True Color (24-bit) support.")
    choice = input("Would you like to enable it now? (y/n): ").strip().lower()
    
    if choice == 'y':
        try:
            with open(bashrc_path, 'a') as f:
                f.write("\n# GPIOnext: Enable True Color for Textual TUI\n")
                f.write("export COLORTERM=truecolor\n")
            os.environ["COLORTERM"] = "truecolor"
            print("True Color enabled in .bashrc and current session.")
            time.sleep(1)
        except Exception as e:
            print(f"Failed to update .bashrc: {e}")
            time.sleep(2)

if __name__ == '__main__':
    check_truecolor()
    parser = argparse.ArgumentParser(description='GPIOnext Configuration Manager')
    parser.add_argument('--pins', default=AVAILABLE_PINS_STRING)
    parser.add_argument('--use_i2c', action='store_true')
    parser.add_argument('--combo_delay', type=int, default=50)
    parser.add_argument('--key_hold_delay', type=int, default=350)
    parser.add_argument('--debounce', type=int, default=1)
    parser.add_argument('--pulldown', action='store_true')
    parser.add_argument('--dev', action='store_true')
    parser.add_argument('--debug', action='store_true')
    args, unknown = parser.parse_known_args()
    
    app = ConfigurationApp(args)
    app.run()