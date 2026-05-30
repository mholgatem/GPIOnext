#!/bin/bash
# GPIOnext Install Script
# Installs GPIOnext to /opt/gpionext with a Python virtualenv and
# pre-built Rust extension binary fetched from GitHub Releases (I'm trying something new!)
#
# Tested on:
#   Raspberry Pi OS Bullseye (32-bit, Debian 11)
#   Raspberry Pi OS Bookworm (32-bit and 64-bit, Debian 12)
#   Pi models: 2B, 3B, 3B+, 4B, 5
#
# Usage: bash install.sh [-noupdate] [--update-core]
#   -noupdate     Skip 'apt-get update' (faster reinstall)
#   --update-core  Only update the Rust binary from GitHub (requires existing install)

set -euo pipefail

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

INSTALL_PATH="/opt/gpionext"
SERVICE_NAME="gpionext"
SERVICE_FILE="/lib/systemd/system/${SERVICE_NAME}.service"
UDEV_RULE="/etc/udev/rules.d/10-gpionext.rules"
CLI_BIN="/usr/bin/gpionext"
GITHUB_REPO="mholgatem/GPIOnext"

NONE='\033[00m'
CYAN='\033[36m'
GREEN='\033[32m'
RED='\033[31m'
FUSCHIA='\033[35m'
UNDERLINE='\033[4m'
BOLD='\033[1m'

# ---------------------------------------------------------------------------
# Flags
# ---------------------------------------------------------------------------

SKIP_APT_UPDATE=false
ONLY_UPDATE_CORE=false

for arg in "$@"; do
    case $arg in
        --noaptupdate)      SKIP_APT_UPDATE=true ;;
        --update-core)     ONLY_UPDATE_CORE=true ;;
    esac
done

# ---------------------------------------------------------------------------
# Root check
# ---------------------------------------------------------------------------

if [ "$(whoami)" != "root" ]; then
    echo "Switching to root user..."
    sudo bash "$0" "$@"
    exit $?
fi

if $ONLY_UPDATE_CORE && [ ! -d "$INSTALL_PATH" ]; then
    echo -e "${RED}ERROR: --update-core requires an existing installation at ${INSTALL_PATH}${NONE}"
    exit 1
fi

SCRIPT=$(readlink -f "$0")
SCRIPTPATH=$(dirname "$SCRIPT")
# Resolve the repo root (two levels up from setup/stock/)
REPOPATH=$(dirname "$(dirname "$SCRIPTPATH")")
cd "$REPOPATH"

# ---------------------------------------------------------------------------
# Architecture detection
# ---------------------------------------------------------------------------

ARCH=$(uname -m)
case "$ARCH" in
    armv7l)  RUST_ARCH="armv7l"   ;;  # Pi 2B / 3 / 4 (32-bit OS)
    aarch64) RUST_ARCH="aarch64"  ;;  # Pi 3 / 4 / 5 (64-bit OS)
    x86_64)  RUST_ARCH="x86_64"   ;;  # Desktop Linux (dev/testing)
    *)
        echo -e "${RED}Unsupported architecture: $ARCH${NONE}"
        echo "Supported: armv7l (Pi 2B-4 32-bit), aarch64 (Pi 3-5 64-bit), x86_64"
        exit 1
        ;;
esac

echo -e "${CYAN}${BOLD}GPIOnext Installer${NONE}"
echo -e "Architecture: ${FUSCHIA}$ARCH${NONE} → binary: gpionext_core-${RUST_ARCH}.so"
echo

# ---------------------------------------------------------------------------
# Core Update Only Path
# ---------------------------------------------------------------------------

if $ONLY_UPDATE_CORE; then
    echo -e "${CYAN}${BOLD}Updating Rust extension binary only...${NONE}"
    
    BINARY_NAME="gpionext_core-${RUST_ARCH}.so"
    DEST="${INSTALL_PATH}/${BINARY_NAME}"

    LATEST_TAG=$(curl -sf "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" \
        | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/') || LATEST_TAG=""

    if [ -z "$LATEST_TAG" ]; then
        echo -e "${RED}Could not determine latest release tag.${NONE}"
        exit 1
    fi

    INSTALLED_VERSION=""
    if [ -f "${INSTALL_PATH}/VERSION" ]; then
        INSTALLED_VERSION=$(cat "${INSTALL_PATH}/VERSION")
    fi

    if [ -n "$INSTALLED_VERSION" ] && [ "$INSTALLED_VERSION" = "$LATEST_TAG" ]; then
        echo -e "${GREEN}Already on the latest version (${LATEST_TAG}). No update needed.${NONE}"
        exit 0
    fi

    BINARY_URL="https://github.com/${GITHUB_REPO}/releases/download/${LATEST_TAG}/${BINARY_NAME}"
    echo "Downloading $BINARY_URL..."
    if curl -sfL "$BINARY_URL" -o "$DEST"; then
        chmod 755 "$DEST"
        ln -sf "$DEST" "${INSTALL_PATH}/gpionext_core.so"
        echo -e "${GREEN}Binary updated successfully to ${LATEST_TAG}.${NONE}"
        VERSION_URL="https://github.com/${GITHUB_REPO}/releases/download/${LATEST_TAG}/VERSION"
        if curl -sfL "$VERSION_URL" -o "${INSTALL_PATH}/VERSION"; then
            echo -e "${GREEN}Version: $(cat "${INSTALL_PATH}/VERSION")${NONE}"
        else
            echo -e "${RED}Warning: could not download VERSION file.${NONE}"
        fi
        echo -e "Restarting ${CYAN}${SERVICE_NAME}${NONE} service..."
        systemctl restart "$SERVICE_NAME"
        exit 0
    else
        echo -e "${RED}Binary download failed.${NONE}"
        exit 1
    fi
fi

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
if [ -f "${REPOPATH}/apt-packages.txt" ]; then
    xargs -a "${REPOPATH}/apt-packages.txt" apt-get -y install
else
    echo -e "${RED}Warning: apt-packages.txt not found. Skipping batch install.${NONE}"
fi

# Core deps (libgpiod soname changed: libgpiod2 on Bullseye, libgpiod3 on Bookworm+)
LIBGPIOD_RT="libgpiod2"
if $IS_BOOKWORM; then
    LIBGPIOD_RT="libgpiod3"
fi
apt-get -y install "${LIBGPIOD_RT}"

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
python3 -m venv "${INSTALL_PATH}/venv"

# Activate venv and install Python packages
"${INSTALL_PATH}/venv/bin/pip" install --quiet --upgrade pip

if [ -f "${REPOPATH}/requirements.txt" ]; then
    "${INSTALL_PATH}/venv/bin/pip" install --quiet -r "${REPOPATH}/requirements.txt"
else
    echo -e "${RED}Warning: requirements.txt not found. Skipping pip install.${NONE}"
fi

# ---------------------------------------------------------------------------
# Download pre-built Rust extension binary from GitHub Releases
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Downloading Rust extension binary...${NONE}"

BINARY_NAME="gpionext_core-${RUST_ARCH}.so"
DEST="${INSTALL_PATH}/${BINARY_NAME}"

# Fetch the latest release tag from GitHub API
LATEST_TAG=$(curl -sf "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" \
    | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/') || LATEST_TAG=""

if [ -z "$LATEST_TAG" ]; then
    echo -e "${RED}Could not determine latest release tag.${NONE}"
    echo "Check your internet connection or visit:"
    echo "  https://github.com/${GITHUB_REPO}/releases"
    echo
    echo "If you have a binary, place it at: ${DEST}"
    BINARY_OK=false
else
    BINARY_URL="https://github.com/${GITHUB_REPO}/releases/download/${LATEST_TAG}/${BINARY_NAME}"
    echo "Downloading $BINARY_URL..."
    if curl -sfL "$BINARY_URL" -o "$DEST"; then
        chmod 755 "$DEST"
        # Create a stable symlink so Python can find it regardless of arch suffix
        ln -sf "$DEST" "${INSTALL_PATH}/gpionext_core.so"
        echo -e "${GREEN}Binary downloaded successfully.${NONE}"
        VERSION_URL="https://github.com/${GITHUB_REPO}/releases/download/${LATEST_TAG}/VERSION"
        if curl -sfL "$VERSION_URL" -o "${INSTALL_PATH}/VERSION"; then
            echo -e "${GREEN}Version: $(cat "${INSTALL_PATH}/VERSION")${NONE}"
        else
            echo -e "${RED}Warning: could not download VERSION file.${NONE}"
        fi
        BINARY_OK=true
    else
        echo -e "${RED}Binary download failed for arch ${RUST_ARCH}.${NONE}"
        echo "You may need to compile from source. See CLAUDE.md for instructions."
        BINARY_OK=false
    fi
fi

# ---------------------------------------------------------------------------
# udev rule (SDL2 / emulator compatibility)
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Installing udev rule...${NONE}"
echo 'KERNEL=="event*", ATTRS{idVendor}=="9999", ATTRS{idProduct}=="8888", MODE:="0644"' \
    > "$UDEV_RULE"
udevadm control --reload-rules
udevadm trigger

# ---------------------------------------------------------------------------
# I2C activation (Raspberry Pi only)
# ---------------------------------------------------------------------------

if [ -f /usr/bin/raspi-config ]; then
    echo -e "${CYAN}${UNDERLINE}Activating I2C interface...${NONE}"
    raspi-config nonint do_i2c 0 || true
fi

# ---------------------------------------------------------------------------
# Kernel modules
# ---------------------------------------------------------------------------

grep -qxF 'uinput' /etc/modules || echo 'uinput' >> /etc/modules
grep -qxF 'evdev'  /etc/modules || echo 'evdev'  >> /etc/modules
grep -qxF 'i2c-dev' /etc/modules || echo 'i2c-dev' >> /etc/modules
modprobe uinput 2>/dev/null || true
modprobe evdev  2>/dev/null || true
modprobe i2c-dev 2>/dev/null || true

# ---------------------------------------------------------------------------
# systemd service
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Installing systemd service...${NONE}"
cp "${REPOPATH}/gpionext.service" "$SERVICE_FILE"
systemctl daemon-reload
systemctl enable "$SERVICE_NAME"

# ---------------------------------------------------------------------------
# CLI wrapper
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Installing CLI wrapper...${NONE}"
cp "${REPOPATH}/usr-bin-gpionext" "$CLI_BIN"
chmod 755 "$CLI_BIN"

# ---------------------------------------------------------------------------
# retrogame conflict check
# ---------------------------------------------------------------------------

for rg_file in /etc/rc.local /home/pi/.profile; do
    if [ -f "$rg_file" ] && grep -q "retrogame" "$rg_file"; then
        echo
        echo -e "${FUSCHIA}retrogame detected in ${rg_file}.${NONE}"
        read -rp "Disable retrogame on startup? [y/N] " USER_INPUT
        if [[ "$USER_INPUT" =~ ^[Yy] ]]; then
            sed -i '/retrogame/s/^#*/: #/' "$rg_file"
            echo -e "${CYAN}retrogame disabled.${NONE}"
        fi
        break
    fi
done

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------

clear
echo
echo -e "${GREEN}${BOLD}GPIOnext installation complete!${NONE}"
echo
if ! $BINARY_OK; then
    echo -e "${RED}WARNING: Rust extension binary not installed.${NONE}"
    echo "  The daemon will not start until the binary is in place."
    echo "  Download from: https://github.com/${GITHUB_REPO}/releases"
	echo "  Place it at: ${DEST}"
    echo
fi

read -rp $'\e[35m\e[4mRun the configuration tool now?\e[0m [Y/N] ' USER_INPUT
if [[ "$USER_INPUT" =~ ^[Yy] ]]; then
    gpionext config
fi

echo
echo -e "Run ${CYAN}gpionext start${NONE} to start the daemon."
systemctl daemon-reload
systemctl start "$SERVICE_NAME" 2>/dev/null || true
