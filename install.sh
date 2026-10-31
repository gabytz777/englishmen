#!/bin/sh
# Englishmen installer for Linux and macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/gabytz777/englishmen/master/install.sh | sh
#
# It installs a prebuilt `em` when the release has one for this platform, and
# otherwise builds from source with Cargo. No sudo, no system directories: the
# runtime goes under the per-user data directory and `em` is linked into
# ~/.local/bin (the shell profile is updated when needed).
set -eu

REPO="gabytz777/englishmen"
VERSION="${EM_VERSION:-latest}"
FROM_SOURCE=0
NO_PROJECTS=0
OPTIONAL=0

usage() {
    cat <<EOF
Englishmen installer

Usage:
    install.sh [options]

Options:
    --version <tag>    Install a specific release (default: latest)
    --from-source      Build from source even if a prebuilt binary exists
    --no-projects      Do not create the default projects folder
    --optional         Also install optional packages (appimagetool, ~15 MB)
    -h, --help         Show this help

Environment:
    EM_VERSION         Same as --version
    EM_HOME            Install the runtime somewhere other than the default
                       per-user data directory

Linux and macOS only. On Windows, download em-windows-x86_64.exe from:
    https://github.com/$REPO/releases
EOF
}

say() {
    printf '%s\n' "$*"
}

warn() {
    printf 'Warning: %s\n' "$*" >&2
}

die() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || die "this install needs '$1', but it was not found"
}

# download <url> [destination]
download() {
    if command -v curl >/dev/null 2>&1; then
        if [ "$#" -ge 2 ]; then
            curl -fsSL "$1" -o "$2"
        else
            curl -fsSL "$1"
        fi
    elif command -v wget >/dev/null 2>&1; then
        if [ "$#" -ge 2 ]; then
            wget -qO "$2" "$1"
        else
            wget -qO- "$1"
        fi
    else
        die "this install needs curl or wget"
    fi
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || die "--version needs a release tag"
            VERSION="$2"
            shift 2
            ;;
        --from-source)
            FROM_SOURCE=1
            shift
            ;;
        --no-projects)
            NO_PROJECTS=1
            shift
            ;;
        --optional)
            OPTIONAL=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown option '$1' (try --help)"
            ;;
    esac
done

if [ "$VERSION" != "latest" ]; then
    case "$VERSION" in
        v*) ;;
        *) VERSION="v$VERSION" ;;
    esac
fi

os=$(uname -s 2>/dev/null || true)
case "$os" in
    Linux) os=linux ;;
    Darwin) os=macos ;;
    *) die "unsupported operating system '$os' (Windows users: download em-windows-x86_64.exe from https://github.com/$REPO/releases)" ;;
esac

machine=$(uname -m 2>/dev/null || true)
case "$machine" in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) die "unsupported CPU architecture '$machine'" ;;
esac

tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/englishmen.XXXXXX")
cleanup() {
    rm -rf "$tmpdir"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

em_bin=""

if [ "$FROM_SOURCE" -eq 0 ]; then
    if [ "$VERSION" = "latest" ]; then
        release_url="https://api.github.com/repos/$REPO/releases/latest"
    else
        release_url="https://api.github.com/repos/$REPO/releases/tags/$VERSION"
    fi

    say "Looking for a prebuilt em for $os-$arch..."
    if download "$release_url" "$tmpdir/release.json" 2>/dev/null; then
        # Asset names are em-<os>-<arch> (optionally in a .tar.gz). The grep is
        # deliberately narrow so unrelated assets are never selected.
        asset_url=$(grep -o "https://[^\"]*/em-${os}-${arch}[^\"]*" "$tmpdir/release.json" | head -n 1 || true)
    else
        asset_url=""
    fi

    if [ -n "$asset_url" ]; then
        say "Downloading $asset_url"
        case "$asset_url" in
            *.tar.gz|*.tgz)
                download "$asset_url" "$tmpdir/em.tar.gz"
                tar -xzf "$tmpdir/em.tar.gz" -C "$tmpdir"
                em_bin=$(find "$tmpdir" -type f -name em | head -n 1 || true)
                ;;
            *.zip)
                need unzip
                download "$asset_url" "$tmpdir/em.zip"
                unzip -q "$tmpdir/em.zip" -d "$tmpdir"
                em_bin=$(find "$tmpdir" -type f -name em | head -n 1 || true)
                ;;
            *)
                download "$asset_url" "$tmpdir/em"
                em_bin="$tmpdir/em"
                ;;
        esac
        [ -n "$em_bin" ] || die "the downloaded archive did not contain 'em'"
        chmod +x "$em_bin"
    fi
fi

if [ -z "$em_bin" ]; then
    if [ "$FROM_SOURCE" -eq 1 ]; then
        say "Building Englishmen from source..."
    else
        say "No prebuilt binary is available for $os-$arch; building from source..."
    fi

    need git
    need cargo

    if [ "$VERSION" = "latest" ]; then
        say "Cloning https://github.com/$REPO..."
        git clone --depth 1 "https://github.com/$REPO.git" "$tmpdir/src"
    else
        say "Cloning https://github.com/$REPO at $VERSION..."
        git clone --depth 1 --branch "$VERSION" "https://github.com/$REPO.git" "$tmpdir/src"
    fi

    say "Compiling (this can take a minute)..."
    cargo build --release --manifest-path "$tmpdir/src/Cargo.toml"
    em_bin="$tmpdir/src/target/release/em"
    [ -x "$em_bin" ] || die "the build did not produce target/release/em"
fi

say "Installing..."
set -- install --yes
if [ "$NO_PROJECTS" -eq 1 ]; then
    set -- "$@" --no-projects
fi
if [ "$OPTIONAL" -eq 1 ]; then
    set -- "$@" --optional
fi
# Redirect stdin so even an older em release (before --yes existed) falls back
# to the default answer instead of waiting for input.
"$em_bin" "$@" </dev/null

say ""
say "Englishmen is installed."
say ""
say "Open a new terminal and run:"
say "    em --version"
say ""
say "Then try:"
say "    em new MyProject"
say "    em MyProject/main.em"
