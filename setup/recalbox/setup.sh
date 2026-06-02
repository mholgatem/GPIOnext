#!/bin/bash
# GPIOnext Setup Script — Recalbox
#
# Installs GPIOnext to /recalbox/share/system/gpionext.
#
# Recalbox uses a read-only root filesystem; /recalbox/share is the writable
# user data partition. Python is not available (and not needed) — GPIOnext
# ships as two standalone Rust binaries:
#   - gpionext          : the GPIO → uinput daemon
#   - gpionext-config   : the Ratatui TUI configuration tool
#
# NOTE: /recalbox/share is mounted noexec on some Recalbox versions.
# Both binaries are copied to /tmp at startup to work around this.
#
# Usage: bash setup/recalbox/setup.sh [--update]
#   --update   Only refresh binaries from GitHub, restart daemon if running

set -euo pipefail

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

INSTALL_PATH="/recalbox/share/system/gpionext"
BIN_DIR="${INSTALL_PATH}/bin"
GITHUB_REPO="mholgatem/GPIOnext"
PID_FILE="/run/gpionext.pid"
CUSTOM_SH="/recalbox/share/system/custom.sh"
ES_CFG_DIR="/recalbox/share/system/.emulationstation"
ES_CUSTOM_CFG="${ES_CFG_DIR}/es_systems.cfg"
ROMS_DIR="/recalbox/share/roms/gpionext"
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

ONLY_UPDATE=false
for arg in "$@"; do
    case $arg in
        --update) ONLY_UPDATE=true ;;
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

echo -e "${CYAN}${BOLD}GPIOnext Installer — Recalbox${NONE}"
echo -e "Architecture: ${FUSCHIA}$ARCH${NONE} → binaries: *-${RUST_ARCH}"
echo

# ---------------------------------------------------------------------------
# Helper: download a binary from the latest GitHub release
# ---------------------------------------------------------------------------

download_binary() {
    local NAME="$1"      # filename on the release page
    local DEST="$2"      # where to write it
    local LATEST_TAG="$3"

    local URL="https://github.com/${GITHUB_REPO}/releases/download/${LATEST_TAG}/${NAME}"
    echo "Downloading ${URL}..."
    if curl -sfL "$URL" -o "$DEST"; then
        chmod +x "$DEST"
        echo -e "${GREEN}Downloaded ${NAME}${NONE}"
        return 0
    else
        echo -e "${RED}Failed to download ${NAME}${NONE}"
        return 1
    fi
}

# ---------------------------------------------------------------------------
# Fetch latest release tag
# ---------------------------------------------------------------------------

LATEST_TAG=$(curl -sf "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" \
    | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/') || LATEST_TAG=""

if [ -z "$LATEST_TAG" ]; then
    echo -e "${RED}Could not determine latest release tag. Check internet connection.${NONE}"
    LATEST_TAG="unknown"
fi

# ---------------------------------------------------------------------------
# Update-only path: refresh binaries and optionally restart daemon
# ---------------------------------------------------------------------------

if $ONLY_UPDATE; then
    echo -e "${CYAN}${BOLD}Updating GPIOnext binaries...${NONE}"

    INSTALLED_VERSION=""
    [ -f "${INSTALL_PATH}/VERSION" ] && INSTALLED_VERSION=$(cat "${INSTALL_PATH}/VERSION")

    if [ -n "$INSTALLED_VERSION" ] && [ "$INSTALLED_VERSION" = "$LATEST_TAG" ]; then
        echo -e "${GREEN}Already on the latest version (${LATEST_TAG}). No update needed.${NONE}"
        exit 0
    fi

    mkdir -p "$BIN_DIR"
    download_binary "gpionext-${RUST_ARCH}"        "${BIN_DIR}/gpionext"        "$LATEST_TAG" || true
    download_binary "gpionext-config-${RUST_ARCH}" "${BIN_DIR}/gpionext-config" "$LATEST_TAG" || true
    echo "$LATEST_TAG" > "${INSTALL_PATH}/VERSION"

    # Restart daemon if currently running
    if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
        kill "$(cat "$PID_FILE")" && rm -f "$PID_FILE"
        cp "${BIN_DIR}/gpionext" /tmp/gpionext && chmod +x /tmp/gpionext
        /tmp/gpionext \
            --config "${INSTALL_PATH}/config/gpionext.json" \
            --debounce 1 --combo_delay 50 &
        echo $! > "$PID_FILE"
        echo -e "${GREEN}Daemon restarted (PID: $(cat "$PID_FILE"))${NONE}"
    fi

    exit 0
fi

# ---------------------------------------------------------------------------
# Full install
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Setting up directory structure at ${INSTALL_PATH}...${NONE}"
mkdir -p "${INSTALL_PATH}/config"
mkdir -p "$BIN_DIR"
echo "recalbox" > "${INSTALL_PATH}/PLATFORM"

# ---------------------------------------------------------------------------
# Download binaries
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Downloading GPIOnext binaries...${NONE}"

BINARY_OK=false
if [ -n "$LATEST_TAG" ] && [ "$LATEST_TAG" != "unknown" ]; then
    D_OK=true
    download_binary "gpionext-${RUST_ARCH}"        "${BIN_DIR}/gpionext"        "$LATEST_TAG" || D_OK=false
    download_binary "gpionext-config-${RUST_ARCH}" "${BIN_DIR}/gpionext-config" "$LATEST_TAG" || D_OK=false
    if $D_OK; then
        echo "$LATEST_TAG" > "${INSTALL_PATH}/VERSION"
        BINARY_OK=true
    fi
fi

if ! $BINARY_OK; then
    echo -e "${RED}Binary download failed. Place them manually:${NONE}"
    echo "  ${BIN_DIR}/gpionext"
    echo "  ${BIN_DIR}/gpionext-config"
fi

# ---------------------------------------------------------------------------
# Kernel modules
# ---------------------------------------------------------------------------

modprobe uinput  2>/dev/null || true
modprobe evdev   2>/dev/null || true
modprobe i2c-dev 2>/dev/null || true

# ---------------------------------------------------------------------------
# I2C boot configuration
# Recalbox uses /boot/recalbox-user-config.txt for user dtparam overrides.
# /boot is a FAT partition mounted read-only; remount rw to write, then
# restore ro so Recalbox's own mount management is not disrupted.
# ---------------------------------------------------------------------------

enable_i2c_boot_config() {
    local BOOT_CFG="$1"
    local REMOUNTED=false
    local CHANGED=false

    # Remount /boot read-write if needed
    if ! mount | grep -q ' /boot ' 2>/dev/null; then
        mount /boot 2>/dev/null || true
    fi
    if mount | grep '/boot' | grep -q 'ro[,)]'; then
        mount -o remount,rw /boot 2>/dev/null && REMOUNTED=true
    fi

    # Create the file if it doesn't exist yet
    [ -f "$BOOT_CFG" ] || touch "$BOOT_CFG"

    if ! grep -q 'dtparam=i2c_arm=on' "$BOOT_CFG" 2>/dev/null; then
        echo 'dtparam=i2c_arm=on' >> "$BOOT_CFG"
        CHANGED=true
    fi
    if ! grep -q 'dtparam=i2c1=on' "$BOOT_CFG" 2>/dev/null; then
        echo 'dtparam=i2c1=on' >> "$BOOT_CFG"
        CHANGED=true
    fi

    # Restore read-only
    if $REMOUNTED; then
        mount -o remount,ro /boot 2>/dev/null || true
    fi

    if $CHANGED; then
        echo -e "${GREEN}I2C dtparam lines added to ${BOOT_CFG} — reboot required for I2C to work.${NONE}"
    else
        echo -e "${GREEN}I2C already enabled in ${BOOT_CFG}${NONE}"
    fi
}

# Recalbox user config is the correct place for dtparam overrides.
# Fall back to standard config.txt paths for non-Recalbox systems.
if [ -f /boot/recalbox-user-config.txt ] || mount | grep -q '/boot'; then
    enable_i2c_boot_config /boot/recalbox-user-config.txt
elif [ -f /boot/firmware/config.txt ]; then
    enable_i2c_boot_config /boot/firmware/config.txt
else
    echo -e "${FUSCHIA}NOTE: Could not locate boot config.${NONE}"
    echo "  To enable I2C manually, add to /boot/recalbox-user-config.txt:"
    echo "    dtparam=i2c_arm=on"
    echo "    dtparam=i2c1=on"
fi

# ---------------------------------------------------------------------------
# udev rule (SDL2 / emulator compatibility)
# ---------------------------------------------------------------------------

#echo -e "${CYAN}${UNDERLINE}Installing udev rule...${NONE}"
#mkdir -p "$(dirname "$UDEV_RULE")"
#echo 'KERNEL=="event*", ATTRS{idVendor}=="9999", ATTRS{idProduct}=="8888", MODE:="0644"' \
#    > "$UDEV_RULE"
#udevadm control --reload-rules 2>/dev/null || true
#udevadm trigger 2>/dev/null || true

# ---------------------------------------------------------------------------
# Startup hook in custom.sh
# Copies binaries to /tmp before running to work around noexec mount.
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Registering startup hook in ${CUSTOM_SH}...${NONE}"

[ -f "$CUSTOM_SH" ] || touch "$CUSTOM_SH"
chmod 755 "$CUSTOM_SH"

if grep -q '# GPIOnext begin' "$CUSTOM_SH"; then
    echo "Startup hook already present — updating..."
    # Remove old block and rewrite
    sed -i '/# GPIOnext begin/,/# GPIOnext end/d' "$CUSTOM_SH"
fi

cat >> "$CUSTOM_SH" << 'HOOKEOF'

# GPIOnext begin
modprobe uinput  2>/dev/null || true
modprobe evdev   2>/dev/null || true
modprobe i2c-dev 2>/dev/null || true
cp /recalbox/share/system/gpionext/bin/gpionext /tmp/gpionext 2>/dev/null && chmod +x /tmp/gpionext
cp /recalbox/share/system/gpionext/bin/gpionext-config /tmp/gpionext-config 2>/dev/null && chmod +x /tmp/gpionext-config
/tmp/gpionext \
    --config /recalbox/share/system/gpionext/config/gpionext.json \
    --debounce 1 --combo_delay 50 &
echo $! > /run/gpionext.pid
# GPIOnext end
HOOKEOF

echo -e "${GREEN}Startup hook written.${NONE}"

# ---------------------------------------------------------------------------
# EmulationStation custom system entry
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Registering GPIOnext in EmulationStation...${NONE}"
mkdir -p "$ES_CFG_DIR"

ES_BLOCK='<!-- GPIOnext begin -->
<system>
  <name>gpionext</name>
  <fullname>GPIOnext</fullname>
  <path>/recalbox/share/roms/gpionext</path>
  <extension>.sh .SH</extension>
  <command>bash %ROM%</command>
  <platform>linux</platform>
  <theme>custom</theme>
</system>
<!-- GPIOnext end -->'

if [ ! -f "$ES_CUSTOM_CFG" ]; then
    printf '<?xml version="1.0"?>\n<systemList>\n%s\n</systemList>\n' "$ES_BLOCK" > "$ES_CUSTOM_CFG"
elif grep -q '<!-- GPIOnext begin -->' "$ES_CUSTOM_CFG"; then
    echo "ES system entry already present — skipping."
else
    sed -i "s|</systemList>|${ES_BLOCK}\n</systemList>|" "$ES_CUSTOM_CFG"
fi

echo -e "${GREEN}ES system entry written to ${ES_CUSTOM_CFG}${NONE}"

# ---------------------------------------------------------------------------
# ROM .sh shortcuts for EmulationStation
# ---------------------------------------------------------------------------

echo -e "${CYAN}${UNDERLINE}Creating EmulationStation ROM shortcuts...${NONE}"
mkdir -p "$ROMS_DIR"

cat > "${ROMS_DIR}/Start GPIOnext.sh" << 'ROMEOF'
#!/bin/bash
modprobe uinput  2>/dev/null || true
modprobe evdev   2>/dev/null || true
modprobe i2c-dev 2>/dev/null || true
cp /recalbox/share/system/gpionext/bin/gpionext /tmp/gpionext 2>/dev/null && chmod +x /tmp/gpionext
/tmp/gpionext \
    --config /recalbox/share/system/gpionext/config/gpionext.json \
    --debounce 1 --combo_delay 50 &
echo $! > /run/gpionext.pid
sleep 2
ROMEOF

cat > "${ROMS_DIR}/Stop GPIOnext.sh" << 'ROMEOF'
#!/bin/bash
if [ -f /run/gpionext.pid ]; then
    kill "$(cat /run/gpionext.pid)" 2>/dev/null || true
    rm -f /run/gpionext.pid
fi
sleep 2
ROMEOF

cat > "${ROMS_DIR}/Configure GPIOnext.sh" << 'ROMEOF'
#!/bin/bash
cp /recalbox/share/system/gpionext/bin/gpionext-config /tmp/gpionext-config 2>/dev/null && chmod +x /tmp/gpionext-config
/tmp/gpionext-config --config /recalbox/share/system/gpionext/config/gpionext.json
ROMEOF

cat > "${ROMS_DIR}/Update GPIOnext.sh" << 'ROMEOF'
#!/bin/bash
bash /recalbox/share/system/gpionext/setup/recalbox/setup.sh --update
ROMEOF

cat > "${ROMS_DIR}/Remove GPIOnext.sh" << 'ROMEOF'
#!/bin/bash
bash /recalbox/share/system/gpionext/setup/recalbox/remove.sh
ROMEOF

chmod 755 "${ROMS_DIR}/"*.sh
echo -e "${GREEN}ROM shortcuts created in ${ROMS_DIR}${NONE}"

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------

clear
echo
echo -e "${GREEN}${BOLD}GPIOnext installation complete! (Recalbox)${NONE}"
echo
echo -e "Install path: ${CYAN}${INSTALL_PATH}${NONE}"
echo -e "Daemon:       ${CYAN}${BIN_DIR}/gpionext${NONE}"
echo -e "Config tool:  ${CYAN}${BIN_DIR}/gpionext-config${NONE}"
echo

if ! $BINARY_OK; then
    echo -e "${RED}WARNING: Binaries not downloaded. Place them manually at:${NONE}"
    echo "  ${BIN_DIR}/gpionext"
    echo "  ${BIN_DIR}/gpionext-config"
    echo
fi

read -rp $'\e[35m\e[4mStart GPIOnext daemon now?\e[0m [Y/N] ' USER_INPUT
if [[ "$USER_INPUT" =~ ^[Yy] ]]; then
    cp "${BIN_DIR}/gpionext" /tmp/gpionext 2>/dev/null && chmod +x /tmp/gpionext
    /tmp/gpionext \
        --config "${INSTALL_PATH}/config/gpionext.json" \
        --debounce 1 --combo_delay 50 &
    echo $! > "$PID_FILE"
    echo -e "${GREEN}GPIOnext started (PID: $(cat "$PID_FILE"))${NONE}"
fi

echo
echo -e "Use the ${CYAN}GPIOnext${NONE} system in EmulationStation to configure and control the daemon."
echo -e "GPIOnext will start automatically on next boot via ${CYAN}${CUSTOM_SH}${NONE}."
