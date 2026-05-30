#!/bin/bash
# GPIOnext Setup Script — Batocera
#
# Installs GPIOnext to /userdata/system/gpionext with a Python virtualenv and
# a pre-built Rust extension binary fetched from GitHub Releases.
#
# Batocera uses a read-only squashfs root with a /userdata writable overlay.
# systemd is not available; startup is handled via /userdata/system/services/.
# /usr/bin is read-only; commands are invoked directly from the install path.
#
# Usage: bash setup/batocera/setup.sh [--noaptupdate] [--update-core]
#   --noaptupdate   Skip apt-get update (faster reinstall on Debian-based Batocera)
#   --update-core   Only refresh the Rust binary from GitHub

set -euo pipefail

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

INSTALL_PATH="/userdata/system/gpionext"
GITHUB_REPO="mholgatem/GPIOnext"
PID_FILE="/run/gpionext.pid"
SERVICE_SCRIPT="/userdata/system/services/gpionext"
ES_CUSTOM_CFG="/userdata/system/configs/emulationstation/es_systems_custom.cfg"
ROMS_DIR="/userdata/roms/gpionext"
UDEV_RULE="/etc/udev/rules.d/10-gpionext.rules"

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

ONLY_UPDATE_CORE=false
for arg in "$@"; do
    case $arg in
        --update-core) ONLY_UPDATE_CORE=true ;;
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

SCRIPT=$(readlink -f "$0")
SCRIPTPATH=$(dirname "$SCRIPT")
REPOPATH=$(dirname "$(dirname "$SCRIPTPATH")")

# ---------------------------------------------------------------------------
# Architecture detection
# ---------------------------------------------------------------------------

ARCH=$(uname -m)
case "$ARCH" in
    armv7l)  RUST_ARCH="armv7l"   ;;
    aarch64) RUST_ARCH="aarch64"  ;;
    x86_64)  RUST_ARCH="x86_64"   ;;
    *)
        echo -e "${RED}Unsupported architecture: $ARCH${NONE}"
        echo "Supported: armv7l (Pi 2B-4 32-bit), aarch64 (Pi 3-5 64-bit), x86_64"
        exit 1
        ;;
esac

echo -e "${CYAN}${BOLD}GPIOnext Installer — Batocera${NONE}"
echo -e "Architecture: ${FUSCHIA}$ARCH${NONE} → binary: gpionext_core-${RUST_ARCH}.so"
echo

# ---------------------------------------------------------------------------
# Core update only path
# ---------------------------------------------------------------------------

if $ONLY_UPDATE_CORE; then
    echo -e "${CYAN}${BOLD}Updating Rust extension binary only...${NONE}"

    BINARY_NAME="gpionext_core-${RUST_ARCH}.so"
    DEST="${INSTALL_PATH}/${BINARY_NAME}"

    LATEST_TAG=$(curl -sf "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" \
        | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/') || LATEST_TAG=""
    [ -z "$LATEST_TAG" ] && { echo -e "${RED}Could not determine latest release tag.${NONE}"; exit 1; }

    INSTALLED_VERSION=""
    [ -f "${INSTALL_PATH}/VERSION" ] && INSTALLED_VERSION=$(cat "${INSTALL_PATH}/VERSION")

    if [ -n "$INSTALLED_VERSION" ] && [ "$INSTALLED_VERSION" = "$LATEST_TAG" ]; then
        echo -e "${GREEN}Already on the latest version (${LATEST_TAG}). No update needed.${NONE}"
        exit 0
    fi

    BINARY_URL="https://github.com/${GITHUB_REPO}/releases/download/${LATEST_TAG}/${BINARY_NAME}"
    echo "Downloading $BINARY_URL..."
    if curl -sfL "$BINARY_URL" -o "$DEST"; then
        chmod 755 "$DEST"
        ln -sf "$DEST" "${INSTALL_PATH}/gpionext_core.so"
        curl -sfL "https://github.com/${GITHUB_REPO}/releases/download/${LATEST_TAG}/VERSION" \
            -o "${INSTALL_PATH}/VERSION" || true
        echo -e "${GREEN}Binary updated to ${LATEST_TAG}.${NONE}"
        # Restart daemon if running
        if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
            bash "$SERVICE_SCRIPT" stop
            bash "$SERVICE_SCRIPT" start
        fi
        exit 0
    else
        echo -e "${RED}Binary download failed.${NONE}"
        exit 1
    fi
fi

# ---------------------------------------------------------------------------
# Directory structure
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Setting up directory structure at ${INSTALL_PATH}...${NONE}"
mkdir -p "${INSTALL_PATH}/config"
echo "batocera" > "${INSTALL_PATH}/PLATFORM"

# ---------------------------------------------------------------------------
# Python virtualenv
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Creating Python virtualenv...${NONE}"
python3 -m venv "${INSTALL_PATH}/venv"
"${INSTALL_PATH}/venv/bin/pip" install --quiet --upgrade pip

if [ -f "${REPOPATH}/requirements.txt" ]; then
    "${INSTALL_PATH}/venv/bin/pip" install --quiet -r "${REPOPATH}/requirements.txt"
else
    echo -e "${RED}Warning: requirements.txt not found. Skipping pip install.${NONE}"
fi

# ---------------------------------------------------------------------------
# Rust extension binary
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Downloading Rust extension binary...${NONE}"

BINARY_NAME="gpionext_core-${RUST_ARCH}.so"
DEST="${INSTALL_PATH}/${BINARY_NAME}"

LATEST_TAG=$(curl -sf "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" \
    | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/') || LATEST_TAG=""

BINARY_OK=false
if [ -n "$LATEST_TAG" ]; then
    BINARY_URL="https://github.com/${GITHUB_REPO}/releases/download/${LATEST_TAG}/${BINARY_NAME}"
    echo "Downloading $BINARY_URL..."
    if curl -sfL "$BINARY_URL" -o "$DEST"; then
        chmod 755 "$DEST"
        ln -sf "$DEST" "${INSTALL_PATH}/gpionext_core.so"
        curl -sfL "https://github.com/${GITHUB_REPO}/releases/download/${LATEST_TAG}/VERSION" \
            -o "${INSTALL_PATH}/VERSION" 2>/dev/null || true
        echo -e "${GREEN}Binary downloaded: $(cat "${INSTALL_PATH}/VERSION" 2>/dev/null || echo "$LATEST_TAG")${NONE}"
        BINARY_OK=true
    else
        echo -e "${RED}Binary download failed for arch ${RUST_ARCH}.${NONE}"
    fi
else
    echo -e "${RED}Could not determine latest release tag. Check internet connection.${NONE}"
    echo "  Place binary manually at: ${DEST}"
fi

# ---------------------------------------------------------------------------
# Kernel modules (load now; Batocera re-loads on each boot via its own init)
# ---------------------------------------------------------------------------

modprobe uinput  2>/dev/null || true
modprobe evdev   2>/dev/null || true
modprobe i2c-dev 2>/dev/null || true

# ---------------------------------------------------------------------------
# udev rule (SDL2 / emulator compatibility)
# Note: Batocera's /etc is a tmpfs overlay — this rule is active for the
# current session but will not persist across reboots without a custom
# Batocera udev rule script. SDL2 emulators typically detect the virtual
# device by vendor/product ID regardless.
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Installing udev rule (session-only on Batocera)...${NONE}"
mkdir -p "$(dirname "$UDEV_RULE")"
echo 'KERNEL=="event*", ATTRS{idVendor}=="9999", ATTRS{idProduct}=="8888", MODE:="0644"' \
    > "$UDEV_RULE"
udevadm control --reload-rules 2>/dev/null || true
udevadm trigger 2>/dev/null || true

# ---------------------------------------------------------------------------
# Batocera service daemon
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Installing Batocera service...${NONE}"
install -m 755 "${SCRIPTPATH}/gpionext-service" "$SERVICE_SCRIPT"
echo -e "${GREEN}Service installed to ${SERVICE_SCRIPT}${NONE}"

# ---------------------------------------------------------------------------
# EmulationStation custom system entry
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Registering GPIOnext in EmulationStation...${NONE}"
mkdir -p "$(dirname "$ES_CUSTOM_CFG")"

ES_BLOCK='<!-- GPIOnext begin -->
<system>
  <name>gpionext</name>
  <fullname>GPIOnext</fullname>
  <path>/userdata/roms/gpionext</path>
  <extension>.sh .SH</extension>
  <command>bash %ROM%</command>
  <platform>linux</platform>
  <theme>custom</theme>
</system>
<!-- GPIOnext end -->'

if [ ! -f "$ES_CUSTOM_CFG" ]; then
    # Create new file with required systemList wrapper
    printf '<?xml version="1.0"?>\n<systemList>\n%s\n</systemList>\n' "$ES_BLOCK" > "$ES_CUSTOM_CFG"
elif grep -q '<!-- GPIOnext begin -->' "$ES_CUSTOM_CFG"; then
    echo "ES system entry already present — skipping."
else
    # Inject before closing </systemList>
    sed -i "s|</systemList>|${ES_BLOCK}\n</systemList>|" "$ES_CUSTOM_CFG"
fi

echo -e "${GREEN}ES system entry written to ${ES_CUSTOM_CFG}${NONE}"
echo -e "${FUSCHIA}Note: verify path on your Batocera version — restart ES for changes to take effect.${NONE}"

# ---------------------------------------------------------------------------
# ROM .sh files (displayed as launchable titles in ES)
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Creating EmulationStation ROM shortcuts...${NONE}"
mkdir -p "$ROMS_DIR"

# Each script is launched directly by ES; ES suspends while it runs, giving
# the script a raw console/framebuffer context — curses works directly.

cat > "${ROMS_DIR}/Start GPIOnext.sh" << 'ROMEOF'
#!/bin/bash
bash /userdata/system/services/gpionext start
sleep 2
ROMEOF

cat > "${ROMS_DIR}/Stop GPIOnext.sh" << 'ROMEOF'
#!/bin/bash
bash /userdata/system/services/gpionext stop
sleep 2
ROMEOF

cat > "${ROMS_DIR}/Configure GPIOnext.sh" << 'ROMEOF'
#!/bin/bash
/userdata/system/gpionext/venv/bin/python3 \
    /userdata/system/gpionext/python/ui/config_manager.py
ROMEOF

cat > "${ROMS_DIR}/Update GPIOnext.sh" << 'ROMEOF'
#!/bin/bash
curl -sfL https://raw.githubusercontent.com/mholgatem/gpionext-dev/main/install.sh | bash
ROMEOF

cat > "${ROMS_DIR}/Remove GPIOnext.sh" << 'ROMEOF'
#!/bin/bash
bash /userdata/system/gpionext/setup/batocera/remove.sh
ROMEOF

chmod 755 "${ROMS_DIR}/"*.sh
echo -e "${GREEN}ROM shortcuts created in ${ROMS_DIR}${NONE}"

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------

clear
echo
echo -e "${GREEN}${BOLD}GPIOnext installation complete! (Batocera)${NONE}"
echo

if ! $BINARY_OK; then
    echo -e "${RED}WARNING: Rust extension binary not installed.${NONE}"
    echo "  Place it at: ${DEST}"
    echo
fi

read -rp $'\e[35m\e[4mStart GPIOnext now?\e[0m [Y/N] ' USER_INPUT
if [[ "$USER_INPUT" =~ ^[Yy] ]]; then
    bash "$SERVICE_SCRIPT" start
    echo -e "${GREEN}GPIOnext started (PID: $(cat "$PID_FILE" 2>/dev/null || echo '?'))${NONE}"
fi

echo
echo -e "Use the ${CYAN}GPIOnext${NONE} system in EmulationStation to control the daemon."
echo -e "Or run: ${CYAN}bash ${SERVICE_SCRIPT} start|stop|status${NONE}"
