#!/bin/sh
# Install or update runx — https://github.com/bswrundquist/runx
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/bswrundquist/runx/main/install.sh | sh
#
# Environment variables:
#   RUNX_INSTALL_DIR  — install directory (default: /usr/local/bin)
#   RUNX_REPO         — GitHub owner/repo  (default: bswrundquist/runx)
#   RUNX_VERSION      — specific tag to install (default: latest)

set -eu

REPO="${RUNX_REPO:-bswrundquist/runx}"
INSTALL_DIR="${RUNX_INSTALL_DIR:-/usr/local/bin}"
VERSION="${RUNX_VERSION:-}"
BINARY="runx"

# --- helpers ----------------------------------------------------------------

info()  { printf '  %s\n' "$@"; }
err()   { printf 'error: %s\n' "$@" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || err "$1 is required but not found"
}

# --- platform detection -----------------------------------------------------

detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Darwin) OS=darwin ;;
        Linux)  OS=linux  ;;
        *)      err "unsupported OS: $OS" ;;
    esac

    case "$ARCH" in
        x86_64|amd64)   ARCH=amd64  ;;
        aarch64|arm64)  ARCH=arm64  ;;
        *)              err "unsupported architecture: $ARCH" ;;
    esac
}

# --- resolve version --------------------------------------------------------

resolve_version() {
    if [ -n "$VERSION" ]; then
        return
    fi

    # Try gh first (handles auth / rate limits), fall back to curl.
    if command -v gh >/dev/null 2>&1; then
        VERSION=$(gh api "repos/${REPO}/releases/latest" --jq '.tag_name' 2>/dev/null) || true
    fi

    if [ -z "$VERSION" ]; then
        VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')
    fi

    [ -n "$VERSION" ] || err "could not determine latest version (no releases found for ${REPO})"
}

# --- download & install -----------------------------------------------------

install_binary() {
    ASSET_BASE="${BINARY}-${OS}-${ARCH}"

    # Fetch release metadata to find the right asset.
    if command -v gh >/dev/null 2>&1; then
        RELEASE_JSON=$(gh api "repos/${REPO}/releases/tags/${VERSION}" 2>/dev/null) || true
    fi
    if [ -z "${RELEASE_JSON:-}" ]; then
        RELEASE_JSON=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/tags/${VERSION}")
    fi

    # Try asset names in order: bare binary, .tar.gz, .zip
    URL=""
    ASSET_NAME=""
    for candidate in "$ASSET_BASE" "${ASSET_BASE}.tar.gz" "${ASSET_BASE}.tgz" "${ASSET_BASE}.zip"; do
        maybe=$(printf '%s' "$RELEASE_JSON" \
            | grep '"browser_download_url"' \
            | grep "\"${candidate}\"" \
            | head -1 \
            | sed 's/.*"browser_download_url": *"//;s/".*//')
        if [ -n "$maybe" ]; then
            URL="$maybe"
            ASSET_NAME="$candidate"
            break
        fi
    done

    [ -n "$URL" ] || err "no matching asset for ${OS}-${ARCH} in release ${VERSION}"

    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    info "downloading ${ASSET_NAME} (${VERSION})..."

    case "$ASSET_NAME" in
        *.tar.gz|*.tgz)
            curl -fsSL "$URL" | tar -xzf - -C "$TMP_DIR"
            BIN=$(find_binary "$TMP_DIR")
            ;;
        *.zip)
            curl -fsSL -o "$TMP_DIR/_download.zip" "$URL"
            unzip -oq "$TMP_DIR/_download.zip" -d "$TMP_DIR"
            rm -f "$TMP_DIR/_download.zip"
            BIN=$(find_binary "$TMP_DIR")
            ;;
        *)
            curl -fsSL -o "$TMP_DIR/$BINARY" "$URL"
            BIN="$TMP_DIR/$BINARY"
            ;;
    esac

    chmod +x "$BIN"

    # Verify the binary works.
    if ! "$BIN" --version >/dev/null 2>&1; then
        err "downloaded binary failed verification (cannot execute)"
    fi

    # Install.
    mkdir -p "$INSTALL_DIR"
    if [ -w "$INSTALL_DIR" ]; then
        mv "$BIN" "$INSTALL_DIR/$BINARY"
    else
        info "elevated permissions required to install to $INSTALL_DIR"
        sudo mv "$BIN" "$INSTALL_DIR/$BINARY"
    fi
}

# Find the runx binary inside an extracted archive directory.
find_binary() {
    dir="$1"
    # Direct match.
    if [ -f "$dir/$BINARY" ]; then
        printf '%s' "$dir/$BINARY"
        return
    fi
    # One level deep (tarballs often have a top-level directory).
    for d in "$dir"/*/; do
        if [ -f "${d}${BINARY}" ]; then
            printf '%s' "${d}${BINARY}"
            return
        fi
    done
    err "cannot find $BINARY binary in downloaded archive"
}

# --- main -------------------------------------------------------------------

main() {
    need curl

    detect_platform
    resolve_version

    printf 'Installing runx %s (%s-%s) to %s\n' "$VERSION" "$OS" "$ARCH" "$INSTALL_DIR"
    install_binary

    info "installed runx to ${INSTALL_DIR}/${BINARY}"
    info "run 'runx --help' to get started"
}

main
