#!/bin/bash
# Shared helpers for GPIOnext setup scripts.
# Source with: . "${MANAGE_DIR}/setup/lib.sh"

GITHUB_REPO="mholgatem/GPIOnext"

NONE='\033[00m'
CYAN='\033[36m'
GREEN='\033[32m'
RED='\033[31m'
FUSCHIA='\033[35m'
UNDERLINE='\033[4m'
BOLD='\033[1m'

# copy_from_source SRC INSTALL_PATH
#
# Copies all GPIOnext files from a source directory (extracted tarball or
# MANAGE_DIR) into INSTALL_PATH, placing binaries in INSTALL_PATH/bin/.
copy_from_source() {
    local SRC="$1"
    local DST="$2"
    local BIN_DIR="${DST}/bin"

    mkdir -p "$BIN_DIR"

    [ -f "${SRC}/gpionext" ]         && cp "${SRC}/gpionext"         "${BIN_DIR}/gpionext"
    [ -f "${SRC}/gpionext-config" ]  && cp "${SRC}/gpionext-config"  "${BIN_DIR}/gpionext-config"
    [ -f "${SRC}/gpionext_core.so" ] && cp "${SRC}/gpionext_core.so" "${BIN_DIR}/gpionext_core.so"
    [ -f "${SRC}/manage" ]           && cp "${SRC}/manage"           "${DST}/manage"
    [ -d "${SRC}/setup" ]            && cp -r "${SRC}/setup"         "${DST}/setup"
    [ -f "${SRC}/VERSION" ]          && cp "${SRC}/VERSION"          "${DST}/VERSION"

    chmod +x "${BIN_DIR}/gpionext" "${BIN_DIR}/gpionext-config" "${DST}/manage" 2>/dev/null || true

    echo -e "${GREEN}Files installed to ${DST}${NONE}"
}

# download_tarball VERSION ARCH INSTALL_PATH
#
# Downloads gpionext-ARCH.tar.gz for VERSION from GitHub Releases, extracts
# it to a temp directory, then calls copy_from_source to place all files.
# Returns 1 on failure so callers can print a warning and decide whether to abort.
download_tarball() {
    local VERSION="$1"
    local ARCH="$2"
    local INSTALL_PATH="$3"
    local URL="https://github.com/${GITHUB_REPO}/releases/download/${VERSION}/gpionext-${ARCH}.tar.gz"
    local TMPDIR

    echo -e "${CYAN}Downloading ${URL}...${NONE}"
    if ! curl -fL "$URL" -o /tmp/gpionext-release.tar.gz; then
        rm -f /tmp/gpionext-release.tar.gz 2>/dev/null || true
        echo -e "${RED}Download failed: ${URL}${NONE}"
        return 1
    fi

    TMPDIR=$(mktemp -d)
    tar -xzf /tmp/gpionext-release.tar.gz -C "$TMPDIR"
    rm -f /tmp/gpionext-release.tar.gz

    copy_from_source "$TMPDIR" "$INSTALL_PATH"

    rm -rf "$TMPDIR"
    return 0
}
