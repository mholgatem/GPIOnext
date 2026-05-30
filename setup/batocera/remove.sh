#!/bin/bash
# GPIOnext Removal Script — Batocera
#
# Stops the daemon, removes the service, ES config entry, ROM shortcuts,
# and the install directory.
#
# Usage: bash /userdata/system/gpionext/setup/batocera/remove.sh

set -euo pipefail

if [ "$(whoami)" != "root" ]; then
    sudo bash "$0" "$@"
    exit $?
fi

INSTALL_PATH="/userdata/system/gpionext"
SERVICE_SCRIPT="/userdata/system/services/gpionext"
ES_CUSTOM_CFG="/userdata/system/configs/emulationstation/es_systems_custom.cfg"
ROMS_DIR="/userdata/roms/gpionext"
UDEV_RULE="/etc/udev/rules.d/10-gpionext.rules"

CYAN='\033[36m'
GREEN='\033[32m'
RED='\033[31m'
NONE='\033[00m'

echo
echo -e "${RED}Removing GPIOnext (Batocera)...${NONE}"
echo

# ---------------------------------------------------------------------------
# Stop daemon
# ---------------------------------------------------------------------------

if [ -f "$SERVICE_SCRIPT" ]; then
    echo "Stopping gpionext daemon..."
    bash "$SERVICE_SCRIPT" stop 2>/dev/null || true
fi

# ---------------------------------------------------------------------------
# Remove service script
# ---------------------------------------------------------------------------

[ -f "$SERVICE_SCRIPT" ] && rm -f "$SERVICE_SCRIPT" && echo "Removed service: ${SERVICE_SCRIPT}"

# ---------------------------------------------------------------------------
# Remove ES custom system entry
# ---------------------------------------------------------------------------

if [ -f "$ES_CUSTOM_CFG" ] && grep -q '<!-- GPIOnext begin -->' "$ES_CUSTOM_CFG"; then
    echo "Removing GPIOnext entry from ${ES_CUSTOM_CFG}..."
    # Delete lines from <!-- GPIOnext begin --> through <!-- GPIOnext end --> inclusive
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
# Remove udev rule (session-only, but clean up anyway)
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
