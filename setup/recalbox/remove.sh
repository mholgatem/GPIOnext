#!/bin/bash
# GPIOnext Removal Script — Recalbox
#
# Stops the daemon, removes the startup hook, ES config entry, ROM shortcuts,
# and the install directory.
#
# Usage: bash /recalbox/share/system/gpionext/setup/recalbox/remove.sh

set -euo pipefail

if [ "$(whoami)" != "root" ]; then
    sudo bash "$0" "$@"
    exit $?
fi

INSTALL_PATH="/recalbox/share/system/gpionext"
PID_FILE="/run/gpionext.pid"
CUSTOM_SH="/recalbox/share/system/custom.sh"
ES_CUSTOM_CFG="/recalbox/share/system/.emulationstation/es_systems.cfg"
ROMS_DIR="/recalbox/share/roms/gpionext"
UDEV_RULE="/etc/udev/rules.d/10-gpionext.rules"

CYAN='\033[36m'
GREEN='\033[32m'
RED='\033[31m'
NONE='\033[00m'

echo
echo -e "${RED}Removing GPIOnext (Recalbox)...${NONE}"
echo

# ---------------------------------------------------------------------------
# Stop daemon
# ---------------------------------------------------------------------------

if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    echo "Stopping gpionext daemon..."
    kill "$(cat "$PID_FILE")" 2>/dev/null || true
    rm -f "$PID_FILE"
else
    # Fallback: kill by process name
    pkill -f 'gpionext.py' 2>/dev/null || true
fi

# ---------------------------------------------------------------------------
# Remove startup hook from custom.sh
# ---------------------------------------------------------------------------

if [ -f "$CUSTOM_SH" ] && grep -q '# GPIOnext begin' "$CUSTOM_SH"; then
    echo "Removing startup hook from ${CUSTOM_SH}..."
    sed -i '/# GPIOnext begin/,/# GPIOnext end/d' "$CUSTOM_SH"
fi

# ---------------------------------------------------------------------------
# Remove ES custom system entry
# ---------------------------------------------------------------------------

if [ -f "$ES_CUSTOM_CFG" ] && grep -q '<!-- GPIOnext begin -->' "$ES_CUSTOM_CFG"; then
    echo "Removing GPIOnext entry from ${ES_CUSTOM_CFG}..."
    sed -i '/<!-- GPIOnext begin -->/,/<!-- GPIOnext end -->/d' "$ES_CUSTOM_CFG"
fi

# ---------------------------------------------------------------------------
# Remove ROM shortcuts
# ---------------------------------------------------------------------------

if [ -d "$ROMS_DIR" ]; then
    echo -e "Removing ROM shortcuts from ${ROMS_DIR}..."
    rm -rf "$ROMS_DIR"
fi

# ---------------------------------------------------------------------------
# Remove udev rule
# ---------------------------------------------------------------------------

[ -f "$UDEV_RULE" ] && rm -f "$UDEV_RULE"
udevadm control --reload-rules 2>/dev/null || true

# ---------------------------------------------------------------------------
# Remove install directory
# ---------------------------------------------------------------------------

if [ -d "$INSTALL_PATH" ]; then
    echo -e "Removing ${INSTALL_PATH}..."
    rm -rf "$INSTALL_PATH"
fi

echo
echo -e "${GREEN}GPIOnext removed. Restart EmulationStation to update the system list.${NONE}"
