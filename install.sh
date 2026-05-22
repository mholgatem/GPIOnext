#!/bin/bash
# GPIOnext Legacy Bootstrap Installer
# Downloads and extracts the Legacy version of GPIOnext and runs setup.sh

set -euo pipefail

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

INSTALL_PATH="/opt/gpionext"
GITHUB_REPO="mholgatem/GPIOnext"
NONE='\033[00m'
CYAN='\033[36m'
GREEN='\033[32m'
RED='\033[31m'
BOLD='\033[1m'

# ---------------------------------------------------------------------------
# Version Formatting (Always LEGACY for this script)
# ---------------------------------------------------------------------------

VERSION="LEGACY"
UPDATE_MODE=false
PASS_THROUGH_ARGS=()

while [ "$#" -gt 0 ]; do
    case "$1" in
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

echo -e "${CYAN}Creating install directory ${INSTALL_PATH}...${NONE}"
mkdir -p "$INSTALL_PATH"

if $UPDATE_MODE; then
    echo -e "${CYAN}Update mode enabled: preserving user settings...${NONE}"
    rm -rf "$BACKUP_DIR"
    mkdir -p "$BACKUP_DIR"
    if [ -f "$CONFIG_DB_PATH" ]; then
        cp "$CONFIG_DB_PATH" "${BACKUP_DIR}/config.db"
    fi
    if [ -f "$SERVICE_FILE" ]; then
        cp "$SERVICE_FILE" "${BACKUP_DIR}/gpionext.service"
    fi
fi

echo -e "${CYAN}Downloading source tarball for ${VERSION}...${NONE}"
# fetch the master branch
SOURCE_URL="https://github.com/${GITHUB_REPO}/archive/refs/tags/${VERSION}.tar.gz"

if curl -sfL "$SOURCE_URL" -o /tmp/gpionext.tar.gz; then
    echo -e "${CYAN}Extracting to ${INSTALL_PATH}...${NONE}"
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
    if [ -f "${BACKUP_DIR}/gpionext.service" ]; then
        cp "${BACKUP_DIR}/gpionext.service" "$SERVICE_FILE"
        systemctl daemon-reload
    fi
    rm -rf "$BACKUP_DIR"
    echo -e "${GREEN}Update complete: user settings preserved.${NONE}"
fi
