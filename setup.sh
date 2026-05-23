#!/bin/bash
# GPIOnext Legacy Setup Script
# Installs Legacy GPIOnext to /opt/gpionext with a Python virtualenv

set -euo pipefail

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

INSTALL_PATH="/opt/gpionext"
SERVICE_NAME="gpionext"
SERVICE_FILE="/lib/systemd/system/${SERVICE_NAME}.service"
UDEV_RULE="/etc/udev/rules.d/10-gpionext.rules"
CLI_BIN="/usr/bin/gpionext"

NONE='\033[00m'
CYAN='\033[36m'
GREEN='\033[32m'
RED='\033[31m'
FUSCHIA='\033[35m'
UNDERLINE='\033[4m'
BOLD='\033[1m'

# ---------------------------------------------------------------------------
# Root check
# ---------------------------------------------------------------------------

if [ "$(whoami)" != "root" ]; then
    echo "Switching to root user..."
    sudo bash "$0" "$@"
    exit $?
fi

SCRIPT=$(readlink -f "$0")
SCRIPTPATH=$(dirname "$SCRIPT")
cd "$SCRIPTPATH"

echo -e "${CYAN}${BOLD}GPIOnext Legacy Setup${NONE}"
echo

# ---------------------------------------------------------------------------
# Debian version detection
# ---------------------------------------------------------------------------

DEBIAN_VERSION=$(cut -d '.' -f 1 /etc/debian_version 2>/dev/null || echo "11")
IS_BOOKWORM=false
if [ "$DEBIAN_VERSION" -ge 12 ] 2>/dev/null; then
    IS_BOOKWORM=true
fi

echo -e "Debian version: ${FUSCHIA}$DEBIAN_VERSION${NONE} (Bookworm+: $IS_BOOKWORM)"
echo

# ---------------------------------------------------------------------------
# Legacy apt source compatibility (Buster/Bullseye)
# ---------------------------------------------------------------------------

ensure_legacy_apt_source() {
    local buster_repo='deb http://legacy.raspbian.org/raspbian/ buster main contrib non-free rpi'
    local buster_list='/etc/apt/sources.list.d/gpionext-buster-legacy.list'
    local sources_blob=''

    # Collect current apt source declarations for simple text checks.
    if [ -f /etc/apt/sources.list ]; then
        sources_blob+=$(cat /etc/apt/sources.list)
    fi
    if [ -d /etc/apt/sources.list.d ]; then
        sources_blob+=$'\n'
        sources_blob+=$(cat /etc/apt/sources.list.d/*.list 2>/dev/null || true)
    fi

    if [ "$DEBIAN_VERSION" -eq 10 ] 2>/dev/null; then
        if ! grep -Eq 'legacy\.raspbian\.org/raspbian/\s+buster' <<< "$sources_blob"; then
            echo -e "${RED}Detected Debian Buster and no legacy Raspbian source.${NONE}"
            echo "Some old RetroPie images require the archived Buster repo to install packages."
            read -r -p "Add legacy Buster source now? [y/N]: " ADD_BUSTER_REPO
            if [[ "$ADD_BUSTER_REPO" =~ ^[Yy]$ ]]; then
                printf '%s\n' "$buster_repo" > "$buster_list"
                echo -e "${GREEN}Added legacy source:${NONE} $buster_repo"
            else
                echo -e "${RED}Skipping legacy source addition by user choice.${NONE}"
            fi
        fi
    elif [ "$DEBIAN_VERSION" -eq 11 ] 2>/dev/null; then
        if ! grep -Eq 'raspbian\.org/raspbian/\s+bullseye|raspbian\.raspberrypi\.org/raspbian/\s+bullseye' <<< "$sources_blob"; then
            echo -e "${RED}Detected Debian Bullseye but no obvious Bullseye Raspbian source was found.${NONE}"
            echo "If apt update fails, review /etc/apt/sources.list and /etc/apt/sources.list.d/*.list."
        fi
    fi
}

# ---------------------------------------------------------------------------
# apt update
# ---------------------------------------------------------------------------

shopt -s nocasematch
if [[ "${1:-}" != "--noaptupdate" ]]; then
	ensure_legacy_apt_source
    echo -e "${CYAN}${UNDERLINE}Updating package lists...${NONE}"
    apt-get update -q
fi
shopt -u nocasematch

# ---------------------------------------------------------------------------
# apt dependencies
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Installing system dependencies...${NONE}"

# Install packages from apt-packages.txt
if [ -f "${SCRIPTPATH}/apt-packages.txt" ]; then
    xargs -a "${SCRIPTPATH}/apt-packages.txt" apt-get -y install
else
    apt-get -y install python3 python3-pip python3-dev python3-venv gcc sqlite3 joystick python3-evdev
fi

# GPIO library: rpi-lgpio for Bookworm+ (Pi 5 compatible), RPi.GPIO for older
if $IS_BOOKWORM; then
    echo -e "Installing ${CYAN}rpi-lgpio${NONE} (Bookworm / Pi 5 compatible)..."
    apt-get -y remove python3-rpi.gpio 2>/dev/null || true
    apt-get -y install python3-rpi-lgpio
else
    echo -e "Installing ${CYAN}RPi.GPIO${NONE} (Bullseye)..."
    apt-get -y install python3-rpi.gpio
fi

# ---------------------------------------------------------------------------
# Create install directory
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Setting up directory structure at ${INSTALL_PATH}...${NONE}"
mkdir -p "${INSTALL_PATH}/config"

# ---------------------------------------------------------------------------
# Python virtualenv
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Creating Python virtualenv...${NONE}"
# Use --system-site-packages so we can use the apt-installed evdev and RPi.GPIO
python3 -m venv --system-site-packages "${INSTALL_PATH}/venv"

# Activate venv and install Python packages
"${INSTALL_PATH}/venv/bin/pip" install --quiet --upgrade pip

if [ -f "${SCRIPTPATH}/requirements.txt" ] && [ -s "${SCRIPTPATH}/requirements.txt" ]; then
    "${INSTALL_PATH}/venv/bin/pip" install --quiet -r "${SCRIPTPATH}/requirements.txt"
fi

# ---------------------------------------------------------------------------
# Copy files to install path
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Updating file permissions...${NONE}"
chmod 755 "${INSTALL_PATH}/gpionext.py"
chmod 755 "${INSTALL_PATH}/config_manager.py"

# ---------------------------------------------------------------------------
# udev rule (SDL2 / emulator compatibility)
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Installing udev rule...${NONE}"
echo 'KERNEL=="event*", ATTRS{idVendor}=="9999", ATTRS{idProduct}=="8888", MODE:="0644"' \
    > "$UDEV_RULE"
udevadm control --reload-rules
udevadm trigger

# ---------------------------------------------------------------------------
# Modules (uinput, evdev)
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Ensuring kernel modules are loaded...${NONE}"
for mod in uinput evdev; do
    if ! grep -q "^$mod" /etc/modules; then
        echo "$mod" >> /etc/modules
    fi
    modprobe "$mod" || true
done

# ---------------------------------------------------------------------------
# Systemd service
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Configuring systemd service...${NONE}"
cp "${INSTALL_PATH}/gpionext.service" "$SERVICE_FILE"
systemctl daemon-reload
systemctl enable "$SERVICE_NAME"

# ---------------------------------------------------------------------------
# CLI Command
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Installing CLI command...${NONE}"
cp "${INSTALL_PATH}/usr-bin-gpionext" "$CLI_BIN"
chmod 755 "$CLI_BIN"

# ---------------------------------------------------------------------------
# Finalize
# ---------------------------------------------------------------------------

echo
echo -e "${GREEN}${BOLD}Installation Successful!${NONE}"
echo -e "You can now run ${CYAN}gpionext config${NONE} to set up your controls."
echo -e "Logs are available via ${CYAN}gpionext journal${NONE}."
echo
