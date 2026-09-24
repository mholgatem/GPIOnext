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
YELLOW='\033[33m'

# ensure_apt_packages [--required PKG...] [--optional PKG...]
#
# Installs any of the listed Debian packages that are not already installed.
# apt is skipped entirely when everything is present, so hosts with a broken
# or offline apt config (e.g. Buster pointing at the retired
# raspbian.raspberrypi.org archive) can still install GPIOnext.
#
# Parameters:
#   --required PKG...  packages the install cannot continue without
#   --optional PKG...  packages that only add convenience (warn if missing)
#
# Returns:
#   0 when all required packages are installed afterwards (optional ones may
#   still be missing, with a warning); 1 when a required package is missing.
ensure_apt_packages() {
    local mode="required" pkg
    local -a required=() optional=() missing=() still_missing=()

    for pkg in "$@"; do
        case "$pkg" in
            --required) mode="required" ;;
            --optional) mode="optional" ;;
            *) if [ "$mode" = "required" ]; then required+=("$pkg"); else optional+=("$pkg"); fi ;;
        esac
    done

    # dpkg-query reports "install ok installed" only for fully installed packages
    for pkg in "${required[@]}" "${optional[@]}"; do
        if ! dpkg-query -W -f='${Status}' "$pkg" 2>/dev/null | grep -q "install ok installed"; then
            missing+=("$pkg")
        fi
    done

    if [ ${#missing[@]} -eq 0 ]; then
        echo -e "${GREEN}System dependencies already installed — skipping apt.${NONE}"
        return 0
    fi

    echo -e "${CYAN}${UNDERLINE}Updating package lists...${NONE}"
    local update_log
    update_log=$(mktemp)
    # A failed update is not fatal: the local package cache may still be usable
    if ! apt-get update -q 2>&1 | tee "$update_log"; then
        echo -e "${YELLOW}Warning: apt-get update reported errors. Trying to install with the existing package lists.${NONE}"
        if grep -q "raspbian.raspberrypi.org" "$update_log"; then
            echo -e "${YELLOW}Your Raspbian Buster archive has moved. Replace it in /etc/apt/sources.list with:${NONE}"
            echo -e "  sudo sed -i 's#raspbian.raspberrypi.org#legacy.raspbian.org#g' /etc/apt/sources.list"
        fi
    fi
    rm -f "$update_log"

    echo -e "${CYAN}${UNDERLINE}Installing system dependencies: ${missing[*]}${NONE}"
    apt-get -y install "${missing[@]}" || true

    for pkg in "${missing[@]}"; do
        if ! dpkg-query -W -f='${Status}' "$pkg" 2>/dev/null | grep -q "install ok installed"; then
            still_missing+=("$pkg")
        fi
    done

    local fatal=0
    for pkg in "${still_missing[@]}"; do
        if [[ " ${required[*]} " == *" $pkg "* ]]; then
            echo -e "${RED}Error: required package '${pkg}' could not be installed.${NONE}" >&2
            fatal=1
        else
            echo -e "${YELLOW}Warning: optional package '${pkg}' could not be installed; continuing without it.${NONE}"
        fi
    done
    return $fatal
}

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
