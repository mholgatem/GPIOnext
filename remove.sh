#!/bin/bash
# GPIOnext Removal Script
# Stops and disables the daemon, removes all installed files.
#
# Usage: bash /opt/gpionext/remove.sh

set -euo pipefail

if [ "$(whoami)" != "root" ]; then
    sudo bash "$0" "$@"
    exit $?
fi

INSTALL_PATH="/opt/gpionext"
SERVICE_NAME="gpionext"
SERVICE_FILE="/lib/systemd/system/${SERVICE_NAME}.service"
UDEV_RULE="/etc/udev/rules.d/10-gpionext.rules"
CLI_BIN="/usr/bin/gpionext"

CYAN='\033[36m'
GREEN='\033[32m'
RED='\033[31m'
NONE='\033[00m'

echo
echo -e "${RED}Removing GPIOnext...${NONE}"
echo

# ---------------------------------------------------------------------------
# Stop and disable systemd service
# ---------------------------------------------------------------------------

if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
    echo "Stopping ${SERVICE_NAME} service..."
    systemctl stop "$SERVICE_NAME"
fi

if systemctl is-enabled --quiet "$SERVICE_NAME" 2>/dev/null; then
    echo "Disabling ${SERVICE_NAME} service..."
    systemctl disable "$SERVICE_NAME"
fi

[ -f "$SERVICE_FILE" ] && rm -f "$SERVICE_FILE"
systemctl daemon-reload

# ---------------------------------------------------------------------------
# Remove udev rule
# ---------------------------------------------------------------------------

[ -f "$UDEV_RULE" ] && rm -f "$UDEV_RULE"
udevadm control --reload-rules 2>/dev/null || true

# ---------------------------------------------------------------------------
# Remove CLI wrapper
# ---------------------------------------------------------------------------

[ -f "$CLI_BIN" ] && rm -f "$CLI_BIN"

# ---------------------------------------------------------------------------
# Remove install directory
# ---------------------------------------------------------------------------

if [ -d "$INSTALL_PATH" ]; then
    echo -e "Removing ${INSTALL_PATH}..."
    rm -rf "$INSTALL_PATH"
fi

# ---------------------------------------------------------------------------
# Offer to re-enable retrogame
# ---------------------------------------------------------------------------

for rg_file in /etc/rc.local /home/pi/.profile; do
    if [ -f "$rg_file" ] && grep -q "retrogame" "$rg_file"; then
        echo
        echo -e "${CYAN}retrogame detected in ${rg_file}.${NONE}"
        read -rp "Re-enable retrogame on startup? [y/n] " USER_INPUT
        if [[ "$USER_INPUT" =~ ^[Yy] ]]; then
            sed -i '/retrogame/s/^: #//' "$rg_file"
            echo -e "${CYAN}retrogame re-enabled.${NONE}"
        fi
        break
    fi
done

echo
echo -e "${GREEN}GPIOnext removed.${NONE}"
