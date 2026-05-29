#!/bin/bash
# GPIOnext Bootstrap Installer
# Downloads and extracts the requested version of GPIOnext and runs setup.sh

set -euo pipefail

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

INSTALL_PATH="/opt/gpionext"
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
    
    # 1. Back up the critical database file
    if [ -f "$CONFIG_DB_PATH" ]; then
        cp "$CONFIG_DB_PATH" "${BACKUP_DIR}/config.db"
    fi
    
    # 2. Extract and parse flags from the old service file before we delete it
    OLD_FLAGS=""
    if [ -f "$SERVICE_FILE" ]; then
        echo "Extracting runtime flags from the legacy systemd configuration..."
        # Extract the text after gpionext.py (handles both local and absolute paths)
        OLD_EXEC_START=$(grep -E '^ExecStart=' "$SERVICE_FILE" | head -n 1)
        
        # Use regex to strip off the prefix and capture everything after gpionext.py
        if [[ "$OLD_EXEC_START" =~ gpionext\.py[[:space:]]+(.*)$ ]]; then
            OLD_FLAGS="${BASH_REMATCH[1]}"
            echo "Found existing runtime flags: $OLD_FLAGS"
        fi
        
        # Back up the raw service file just in case the user wants a fallback reference
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
else
    echo -e "${RED}Error: Download failed for version ${VERSION}.${NONE}"
    exit 1
fi

# ---------------------------------------------------------------------------
# Hand-off to setup.sh
# ---------------------------------------------------------------------------

if [ -f "${INSTALL_PATH}/setup.sh" ]; then
    echo -e "${GREEN}Handing off to setup.sh...${NONE}"
    bash "${INSTALL_PATH}/setup.sh" "${PASS_THROUGH_ARGS[@]}"
else
    echo -e "${RED}Error: setup.sh not found in extracted source.${NONE}"
    exit 1
fi

if $UPDATE_MODE; then
    echo -e "${CYAN}Restoring preserved settings...${NONE}"
    if [ -f "${BACKUP_DIR}/config.db" ]; then
        mkdir -p "${INSTALL_PATH}/config"
        cp "${BACKUP_DIR}/config.db" "$CONFIG_DB_PATH"
    fi
    
    echo -e "${CYAN}Configuring systemd service...${NONE}"

    # add flags from old service file to new service file
    NEW_SERVICE_TEMPLATE="${INSTALL_PATH}/gpionext.service"

    if [ -f "$NEW_SERVICE_TEMPLATE" ]; then
        # If we found old runtime flags from the backup phase, splice them into the template
        if [ -n "${OLD_FLAGS:-}" ]; then
            echo "Replacing template default flags with your saved runtime configurations..."
            
            # Escape special tokens safely for sed execution blocks
            SAFE_FLAGS=$(echo "$OLD_FLAGS" | sed 's/[&/\]/\\&/g')
            
            # This regex matches 'gpionext.py' and clears out everything remaining on that line,
            # replacing it strictly with 'gpionext.py' plus your custom extracted configuration flags.
            sed -i "s|gpionext\.py.*$|gpionext.py $SAFE_FLAGS|g" "$NEW_SERVICE_TEMPLATE"
        fi

        # Copy the customized file to the systemd runtime location
        cp "$NEW_SERVICE_TEMPLATE" "$SERVICE_FILE"
        
        systemctl daemon-reload
        systemctl enable "$SERVICE_NAME"
    else
        echo -e "${RED}Error: New service template configuration file not found.${NONE}"
        exit 1
    fi

    # Restore your database after the file tree has been updated
    if $UPDATE_MODE && [ -f "${BACKUP_DIR}/config.db" ]; then
        echo "Restoring configuration database..."
        cp "${BACKUP_DIR}/config.db" "$CONFIG_DB_PATH"
    fi
    
    rm -rf "$BACKUP_DIR"
    echo -e "${GREEN}Update complete: user settings preserved.${NONE}"
fi
