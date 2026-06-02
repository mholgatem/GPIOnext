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

# download_tarball VERSION ARCH INSTALL_PATH
#
# Downloads gpionext-ARCH.tar.gz for VERSION from GitHub Releases, extracts
# binaries to INSTALL_PATH/bin/ and writes VERSION to INSTALL_PATH/VERSION.
# Returns 1 on failure so callers can print a warning and decide whether to abort.
download_tarball() {
    local VERSION="$1"
    local ARCH="$2"
    local INSTALL_PATH="$3"
    local BIN_DIR="${INSTALL_PATH}/bin"
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
    mkdir -p "$BIN_DIR"

    [ -f "${TMPDIR}/gpionext" ]         && cp "${TMPDIR}/gpionext"         "${BIN_DIR}/gpionext"
    [ -f "${TMPDIR}/gpionext-config" ]  && cp "${TMPDIR}/gpionext-config"  "${BIN_DIR}/gpionext-config"
    [ -f "${TMPDIR}/gpionext_core.so" ] && cp "${TMPDIR}/gpionext_core.so" "${BIN_DIR}/gpionext_core.so"
    [ -f "${TMPDIR}/VERSION" ]           && cp "${TMPDIR}/VERSION"           "${INSTALL_PATH}/VERSION"

    chmod +x "${BIN_DIR}/gpionext" "${BIN_DIR}/gpionext-config" 2>/dev/null || true
    rm -rf "$TMPDIR" /tmp/gpionext-release.tar.gz

    echo -e "${GREEN}Binaries installed to ${BIN_DIR}${NONE}"
    return 0
}
