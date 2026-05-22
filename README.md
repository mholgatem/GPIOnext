<h1>GPIOnext</h1>
<h6>A Python Based GPIO Controller</h6>

<h6> *** Now Compatible with Raspberry Pi 5 (Bookworm)!*** </h6>

This is a GPIO controller that is fully compatible with RetroPie (and PiPlay). For anyone that is familiar with Adafruit's RetroGame Utility, this is very similar. The main difference being that this is user friendly and full featured.
<h4>What's New?</h4>
<ul><li>Configuration tool to auto map buttons to keystrokes</li>
<li>Graphical Command line interface allows you to configure controls even on "lite" OS's</li>
<li>supports button combinations for additional keystrokes</li>
<li>map multiple keystrokes/commands to a single button</li>
<li><b>It supports system commands! (you can map volume/shutdown/etc to buttons)</b></li>
</ul>
<h4>How to install</h4>in terminal type:
```bash
curl -sfL https://raw.githubusercontent.com/mholgatem/gpionext-dev/main/install.sh | sudo bash -s -- --version LEGACY
```
That's it! The installer is still very much in the beta stage, so let me know if you have problems. But I have tested it on several clean raspbian/piplay images with no problem.

### Basic Setup
```bash
gpionext config
```
This interactive tool will guide you through:
- Detecting pressed pins.
- Mapping pins to "Commands", "Keys", or "Joypad Buttons/Axes".
- Setting up multi-button combos.

### Peripheral Types
- **Button:** Triggers a standard joystick button (e.g., Button A, Start).
- **Key:** Triggers a keyboard key with auto-repeat.
- **Axis:** Maps pins to analog joystick directions (Up/Down/Left/Right).
- **Command:** Executes a shell command when the button is pressed.

---

## CLI Commands & Settings

GPIOnext provides a powerful CLI wrapper via the `gpionext` command.

### Daemon Management
- `gpionext start`: Enable and start the background daemon.
- `gpionext stop`: Stop the daemon.
- `gpionext reload`: Send SIGHUP to the daemon to hot-reload the configuration without a full restart.
- `gpionext disable`: Stop and disable the auto-start service.

### Updates & Removal
- `gpionext update`: Pull the latest source and binary from GitHub.
- `gpionext update --version <version>`: Update to a specific version.
- `gpionext remove`: Completely remove GPIOnext from the system, including `/opt/gpionext`, the systemd service, and udev rules.

### Diagnostics
- `gpionext journal`: Stream live log output from the daemon (Press Ctrl+C to exit).
- `gpionext test [1-4]`: Run `jstest` on one of the four virtual joypads created by GPIOnext.

### Global Settings
Settings are applied immediately and will restart the daemon:
- `gpionext set combo_delay <ms>`: The window (default 50ms) to detect multi-button combos.
- `gpionext set key_hold_delay <ms>`: The delay (default 350ms) before keyboard auto-repeat starts.
- `gpionext set debounce <ms>`: Button debounce time (default 1ms).
- `gpionext set pulldown <true|false>`: Use internal pulldown resistors (default: false/pullup).
- `gpionext set dev <true|false>`: Enable verbose logging to the system journal.
  
