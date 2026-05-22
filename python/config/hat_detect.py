"""
hat_detect.py — Audio HAT detection and GPIO pin reservation.

Audio HATs (HiFiBerry, JustBoom, IQaudio, Pimoroni) claim specific GPIO pins
for I2S audio. When GPIOnext tries to configure those same pins as button
inputs the HAT stops working, often silently. This module detects loaded audio
overlays at daemon startup and removes conflicting pins from the active set
with a clear warning.

Detection strategy (tried in order):
  1. /proc/device-tree/hat/product — EEPROM on the HAT itself (most reliable)
  2. /boot/firmware/config.txt     — Bookworm default location
  3. /boot/config.txt              — Bullseye and earlier

Usage (from gpionext.py):
    from config.hat_detect import detect_audio_hat
    hat = detect_audio_hat()
    if hat:
        print(f"Audio HAT '{hat['name']}' detected — reserving pins {hat['reserved_pins']}")
        args.pins = [p for p in args.pins if p not in hat['reserved_pins']]
"""
import os

# ---------------------------------------------------------------------------
# Known audio HAT overlay names → reserved BOARD pin numbers
# Pins listed are I2S/GPIO lines the HAT uses for audio data.
# Sources: each HAT's datasheet and Raspberry Pi DT overlay source.
# ---------------------------------------------------------------------------

_HAT_PIN_TABLE: dict[str, tuple[int, ...]] = {
    # HiFiBerry DAC / DAC+ / DAC+ Pro
    # I2S: BCK=12, FS=35, DIN=38, DOUT=40; MCLK on Pro: 7
    'hifiberry-dac':        (12, 35, 38, 40),
    'hifiberry-dacplus':    (12, 35, 38, 40),
    'hifiberry-dacplusadc': (12, 35, 38, 40),
    'hifiberry-dacplusdsp': (12, 35, 38, 40),
    # HiFiBerry Digi (digital output via SPDIF, uses same I2S pins)
    'hifiberry-digi':       (12, 35, 38, 40),
    'hifiberry-digi-pro':   (12, 35, 38, 40),
    # HiFiBerry AMP
    'hifiberry-amp':        (12, 35, 38, 40),
    'hifiberry-amp3':       (12, 35, 38, 40),

    # JustBoom DAC HAT / Amp HAT
    'justboom-dac':         (12, 35, 38, 40),
    'justboom-amp':         (12, 35, 38, 40),
    'justboom-digi':        (12, 35, 38, 40),

    # IQaudio DAC / DAC Pro / DigiAMP+
    'iqaudio-dacplus':      (12, 35, 38, 40),
    'iqaudio-dac':          (12, 35, 38, 40),
    'iqaudio-digiampplus':  (12, 35, 38, 40),
    'iqaudio-codec':        (12, 35, 38, 40),

    # Pimoroni Audio HATs
    'pimoroni-audio':       (12, 35, 38, 40),

    # Adafruit MAX98357 I2S breakout
    'hifiberry-dac':        (12, 35, 38, 40),  # uses same I2S

    # Waveshare Audio HAT
    'wm8960-soundcard':     (12, 35, 38, 40),

    # Generic I2S audio (dtoverlay=i2s-mmap, dtoverlay=googlevoicehat-soundcard, etc.)
    'i2s-mmap':             (12, 35, 38, 40),
    'googlevoicehat-soundcard': (12, 35, 38, 40),
}

# Display names for known overlays (for user-facing messages)
_HAT_DISPLAY_NAMES: dict[str, str] = {
    'hifiberry-dac':        'HiFiBerry DAC',
    'hifiberry-dacplus':    'HiFiBerry DAC+',
    'hifiberry-dacplusadc': 'HiFiBerry DAC+ADC',
    'hifiberry-dacplusdsp': 'HiFiBerry DAC+DSP',
    'hifiberry-digi':       'HiFiBerry Digi',
    'hifiberry-digi-pro':   'HiFiBerry Digi+',
    'hifiberry-amp':        'HiFiBerry AMP',
    'hifiberry-amp3':       'HiFiBerry AMP3',
    'justboom-dac':         'JustBoom DAC HAT',
    'justboom-amp':         'JustBoom Amp HAT',
    'justboom-digi':        'JustBoom Digi HAT',
    'iqaudio-dacplus':      'IQaudio DAC+',
    'iqaudio-dac':          'IQaudio DAC',
    'iqaudio-digiampplus':  'IQaudio DigiAMP+',
    'iqaudio-codec':        'IQaudio Codec Zero',
    'pimoroni-audio':       'Pimoroni Audio HAT',
    'wm8960-soundcard':     'Waveshare WM8960',
    'i2s-mmap':             'Generic I2S audio',
    'googlevoicehat-soundcard': 'Google Voice HAT',
}


# ---------------------------------------------------------------------------
# Detection
# ---------------------------------------------------------------------------

def detect_audio_hat() -> dict | None:
    """
    Detect a connected audio HAT and return its name and reserved BOARD pins.

    Tries three detection methods in order of reliability:
      1. HAT EEPROM via /proc/device-tree/hat/product
      2. /boot/firmware/config.txt overlay lines (Bookworm)
      3. /boot/config.txt overlay lines (Bullseye and earlier)

    Returns:
        dict with keys:
            'name'          (str)       : display name e.g. 'HiFiBerry DAC+'
            'overlay'       (str)       : overlay key e.g. 'hifiberry-dacplus'
            'reserved_pins' (list[int]) : BOARD pin numbers in use by the HAT
        or None if no known audio HAT is detected.
    """
    # Method 1: HAT EEPROM (most reliable — always present when a real HAT is seated)
    result = _detect_via_eeprom()
    if result:
        return result

    # Method 2 & 3: /boot config.txt overlay lines
    for config_path in ('/boot/firmware/config.txt', '/boot/config.txt'):
        result = _detect_via_config(config_path)
        if result:
            return result

    return None


def _detect_via_eeprom() -> dict | None:
    """
    Read the HAT product name from the device-tree HAT EEPROM node.
    This is written by the HAT at boot time for officially-certified HATs.

    Returns:
        dict or None
    """
    eeprom_path = '/proc/device-tree/hat/product'
    try:
        with open(eeprom_path, 'rb') as f:
            # EEPROM string is null-terminated
            product = f.read().rstrip(b'\x00').decode('utf-8', errors='replace').lower()
    except OSError:
        return None

    # Match product string against known overlay keys
    for overlay_key in _HAT_PIN_TABLE:
        # e.g. "hifiberry-dacplus" in "hifiberry dacplus stereo"
        if overlay_key.replace('-', ' ') in product.replace('-', ' '):
            return _build_result(overlay_key)

    return None


def _detect_via_config(config_path: str) -> dict | None:
    """
    Scan a Pi boot config file for dtoverlay= lines matching known audio HATs.

    Parameters:
        config_path (str): path to the Pi boot config file

    Returns:
        dict or None
    """
    try:
        with open(config_path, 'r', errors='replace') as f:
            lines = f.readlines()
    except OSError:
        return None

    for line in lines:
        stripped = line.strip()
        # Skip comments and non-overlay lines
        if stripped.startswith('#') or not stripped.startswith('dtoverlay='):
            continue
        # dtoverlay=hifiberry-dacplus,<optional params>
        overlay_value = stripped.split('=', 1)[1].split(',')[0].strip().lower()
        if overlay_value in _HAT_PIN_TABLE:
            return _build_result(overlay_value)

    return None


def _build_result(overlay_key: str) -> dict:
    """
    Build the result dict for a detected overlay.

    Parameters:
        overlay_key (str): key into _HAT_PIN_TABLE

    Returns:
        dict: {'name', 'overlay', 'reserved_pins'}
    """
    return {
        'name':          _HAT_DISPLAY_NAMES.get(overlay_key, overlay_key),
        'overlay':       overlay_key,
        'reserved_pins': list(_HAT_PIN_TABLE[overlay_key]),
    }


# ---------------------------------------------------------------------------
# Warning formatter (called by gpionext.py)
# ---------------------------------------------------------------------------

def format_hat_warning(hat: dict) -> str:
    """
    Format a human-readable startup warning for the detected audio HAT.

    Parameters:
        hat (dict): result from detect_audio_hat()

    Returns:
        str: multi-line warning string ready to print or log
    """
    pins_str = ', '.join(str(p) for p in hat['reserved_pins'])
    return (
        f"[WARNING] Audio HAT detected: {hat['name']}\n"
        f"          Reserved BOARD pins: {pins_str}\n"
        f"          These pins have been removed from GPIOnext's active pin set.\n"
        f"          To use these pins anyway, run: gpionext set pins <your_pin_list>"
    )
