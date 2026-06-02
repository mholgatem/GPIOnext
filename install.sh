#!/bin/bash
# GPIOnext Bootstrap Installer
# Downloads and extracts the requested version of GPIOnext and runs the
# appropriate platform setup script (stock / batocera / recalbox).

set -euo pipefail

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

SERVICE_NAME="gpionext"
GITHUB_REPO="mholgatem/GPIOnext"
NONE='\033[00m'
CYAN='\033[36m'
GREEN='\033[32m'
RED='\033[31m'
FUSCHIA='\033[35m'
UNDERLINE='\033[4m'
BOLD='\033[1m'

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------
# Sets PLATFORM (stock | batocera | recalbox) and INSTALL_PATH accordingly.
# Batocera: squashfs root + /userdata writable overlay, no systemd.
# Recalbox: similar read-only root + /recalbox/share writable, no systemd.
detect_platform() {
    if [ -f /etc/batocera-release ]; then
        PLATFORM="batocera"
        INSTALL_PATH="/userdata/system/gpionext"
    elif [ -d /recalbox/share ]; then
        PLATFORM="recalbox"
        INSTALL_PATH="/recalbox/share/system/gpionext"
    else
        PLATFORM="stock"
        INSTALL_PATH="/opt/gpionext"
    fi
}

detect_platform
echo -e "${CYAN}Detected platform: ${BOLD}${PLATFORM}${NONE}"
echo -e "${CYAN}Install path: ${INSTALL_PATH}${NONE}"

# ---------------------------------------------------------------------------
# Version Formatting
# ---------------------------------------------------------------------------

VERSION=""
UPDATE_MODE=false
PASS_THROUGH_ARGS=()
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            if [ -n "${2:-}" ] && [[ "${2:-}" != --* ]]; then
                VERSION="$2"
                shift 2
            else
                echo -e "${RED}Error: --version requires a value (example: --version v0.3.3).${NONE}"
                exit 1
            fi
            ;;
        --update)
            UPDATE_MODE=true
            shift
            ;;
        *)
            PASS_THROUGH_ARGS+=("$1")
            shift
            ;;
    esac
done

if [ -z "$VERSION" ]; then
    echo -e "${CYAN}Determining latest release...${NONE}"
    VERSION=$(curl -sf "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" \
        | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/') || VERSION=""
    
    if [ -z "$VERSION" ]; then
        echo -e "${RED}Error: Could not determine latest release.${NONE}"
        exit 1
    fi
else
    # Format version: lowercase and prepend 'v' if missing
    # Exception: 'LEGACY' should always be uppercase
    if [[ "${VERSION,,}" == "legacy" ]]; then
        VERSION="LEGACY"
    else
        VERSION="${VERSION,,}"
        if [[ ! "$VERSION" =~ ^v ]]; then
            VERSION="v${VERSION}"
        fi
    fi
fi

echo -e "Target version: ${BOLD}${VERSION}${NONE}"

BACKUP_DIR="/tmp/gpionext-update-backup"
SERVICE_FILE="/lib/systemd/system/gpionext.service"
CONFIG_DB_PATH="${INSTALL_PATH}/config/config.db"
CONFIG_JSON_PATH="${INSTALL_PATH}/config/gpionext.json"

# Detect CPU architecture for binary selection
detect_arch() {
    case "$(uname -m)" in
        armv7l|armv6l) echo "armv7l" ;;
        aarch64)        echo "aarch64" ;;
        x86_64)         echo "x86_64" ;;
        *)              echo "aarch64" ;;  # safe default for Pi
    esac
}
ARCH="$(detect_arch)"

# ---------------------------------------------------------------------------
# Root check
# ---------------------------------------------------------------------------

if [ "$(whoami)" != "root" ]; then
    echo "Switching to root user..."
    sudo bash "$0" "$@"
    exit $?
fi

# ---------------------------------------------------------------------------
# Fetch and Extract
# ---------------------------------------------------------------------------

echo -e "${CYAN}Creating install directory ${INSTALL_PATH} if non-existent...${NONE}"
mkdir -p "$INSTALL_PATH"

if $UPDATE_MODE; then

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
    
    echo -e "${CYAN}Update mode enabled: preserving user configurations...${NONE}"
    rm -rf "$BACKUP_DIR"
    mkdir -p "$BACKUP_DIR"
    
    # 1. Back up config files (JSON takes priority; fall back to legacy DB)
    if [ -f "$CONFIG_JSON_PATH" ]; then
        cp "$CONFIG_JSON_PATH" "${BACKUP_DIR}/gpionext.json"
    fi
    if [ -f "$CONFIG_DB_PATH" ]; then
        cp "$CONFIG_DB_PATH" "${BACKUP_DIR}/config.db"
    fi
    
    # 2. On stock only: extract and preserve runtime flags from the systemd service file.
    OLD_FLAGS=""
    if [ "$PLATFORM" = "stock" ] && [ -f "$SERVICE_FILE" ]; then
        echo "Extracting runtime flags from the legacy systemd configuration..."
        OLD_EXEC_START=$(grep -E '^ExecStart=' "$SERVICE_FILE" | head -n 1)
        if [[ "$OLD_EXEC_START" =~ gpionext\.py[[:space:]]+(.*)$ ]]; then
            OLD_FLAGS="${BASH_REMATCH[1]}"
            echo "Found existing runtime flags: $OLD_FLAGS"
        fi
        cp "$SERVICE_FILE" "${BACKUP_DIR}/gpionext.service.old"
    fi

    # 3. Purge the old installation tree to remove legacy structural clutter
    echo -e "${CYAN}Purging old application directory file assets...${NONE}"
    # Delete everything inside /opt/gpionext EXCEPT the backup directory itself
    find "$INSTALL_PATH" -mindepth 1 -maxdepth 1 ! -name "$(basename "$BACKUP_DIR")" -exec rm -rf {} +
fi

echo -e "${CYAN}Downloading source tarball for ${VERSION}...${NONE}"
SOURCE_URL="https://github.com/${GITHUB_REPO}/archive/refs/tags/${VERSION}.tar.gz"

if curl -sfL "$SOURCE_URL" -o /tmp/gpionext.tar.gz; then

    echo -e "${CYAN}Purging old files from ${INSTALL_PATH}...${NONE}"
    find "$INSTALL_PATH" -mindepth 1 -maxdepth 1 ! -name "venv" ! -name "$(basename "$BACKUP_DIR")" -exec rm -rf {} +
	
    echo -e "${CYAN}Extracting to ${INSTALL_PATH}...${NONE}"
    mkdir -p "$INSTALL_PATH"
    tar -xzf /tmp/gpionext.tar.gz -C "$INSTALL_PATH" --strip-components=1
    rm /tmp/gpionext.tar.gz
    echo "$PLATFORM" > "${INSTALL_PATH}/PLATFORM"
else
    echo -e "${RED}Error: Download failed for version ${VERSION}.${NONE}"
    exit 1
fi

# ---------------------------------------------------------------------------
# Download pre-built Rust binaries from GitHub Releases
# ---------------------------------------------------------------------------

RELEASE_BASE="https://github.com/${GITHUB_REPO}/releases/download/${VERSION}"

echo -e "${CYAN}Downloading gpionext binaries (${ARCH})...${NONE}"
mkdir -p "${INSTALL_PATH}/bin"
if curl -fL "${RELEASE_BASE}/gpionext-${ARCH}.tar.gz" -o /tmp/gpionext-bins.tar.gz; then
    tar -xzf /tmp/gpionext-bins.tar.gz -C "${INSTALL_PATH}/bin"
    chmod +x "${INSTALL_PATH}/bin/gpionext" \
             "${INSTALL_PATH}/bin/gpionext-config"
    rm /tmp/gpionext-bins.tar.gz
    echo -e "${GREEN}Binaries extracted to ${INSTALL_PATH}/bin${NONE}"
else
    echo -e "${FUSCHIA}Warning: could not download gpionext binaries tarball.${NONE}"
fi

# Symlink config manager into PATH if a writable bin directory exists.
# /usr/local/bin doesn't exist on Recalbox/Batocera — skip silently there.
if [ -f "${INSTALL_PATH}/bin/gpionext-config" ]; then
    for BINDIR in /usr/local/bin /usr/bin; do
        if [ -d "$BINDIR" ] && [ -w "$BINDIR" ]; then
            install -m 755 "${INSTALL_PATH}/bin/gpionext-config" "${BINDIR}/gpionext-config" \
                && echo -e "${GREEN}gpionext-config installed to ${BINDIR}${NONE}" \
                && break
        fi
    done
fi

# ---------------------------------------------------------------------------
# Hand-off to platform setup script
# ---------------------------------------------------------------------------

PLATFORM_SETUP="${INSTALL_PATH}/setup/${PLATFORM}/setup.sh"
if [ -f "$PLATFORM_SETUP" ]; then
    echo -e "${GREEN}Handing off to setup/${PLATFORM}/setup.sh...${NONE}"
    bash "$PLATFORM_SETUP" "${PASS_THROUGH_ARGS[@]}"
else
    echo -e "${RED}Error: setup/${PLATFORM}/setup.sh not found in extracted source.${NONE}"
    exit 1
fi

if $UPDATE_MODE; then
    echo -e "${CYAN}Restoring preserved settings...${NONE}"
    mkdir -p "${INSTALL_PATH}/config"

    # Restore JSON config if it was backed up
    if [ -f "${BACKUP_DIR}/gpionext.json" ]; then
        cp "${BACKUP_DIR}/gpionext.json" "$CONFIG_JSON_PATH"
    # Migrate legacy config.db → gpionext.json if JSON not yet created
    elif [ -f "${BACKUP_DIR}/config.db" ]; then
        cp "${BACKUP_DIR}/config.db" "$CONFIG_DB_PATH"
        if command -v sqlite3 &>/dev/null && [ ! -f "$CONFIG_JSON_PATH" ]; then
            echo -e "${CYAN}Migrating config.db → gpionext.json...${NONE}"
            bash "${INSTALL_PATH}/setup/migrate_db_to_json.sh" \
                "$CONFIG_DB_PATH" "$CONFIG_JSON_PATH" \
                || echo -e "${FUSCHIA}Warning: migration failed, keeping config.db.${NONE}"
        fi
    fi

    # On stock only: splice saved runtime flags back into the new service file.
    if [ "$PLATFORM" = "stock" ]; then
        echo -e "${CYAN}Configuring systemd service...${NONE}"
        NEW_SERVICE_TEMPLATE="${INSTALL_PATH}/gpionext.service"
        if [ -f "$NEW_SERVICE_TEMPLATE" ]; then
            if [ -n "${OLD_FLAGS:-}" ]; then
                echo "Replacing template default flags with your saved runtime configurations..."
                SAFE_FLAGS=$(echo "$OLD_FLAGS" | sed 's/[&/\]/\\&/g')
                sed -i "s|gpionext\.py.*$|gpionext.py $SAFE_FLAGS|g" "$NEW_SERVICE_TEMPLATE"
            fi
            cp "$NEW_SERVICE_TEMPLATE" "$SERVICE_FILE"
            systemctl daemon-reload
            systemctl enable "$SERVICE_NAME"
        else
            echo -e "${RED}Error: New service template configuration file not found.${NONE}"
            exit 1
        fi
    fi

    rm -rf "$BACKUP_DIR"
    echo -e "${GREEN}Update complete: user settings preserved.${NONE}"
fi
