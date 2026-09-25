#!/bin/sh
# Installs the `ward` command (Wardscript) on macOS or Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/ahmedelraei/wardscript/main/install/install.sh | sh
#
# Environment:
#   WARD_VERSION          a release tag, like v0.1.0-beta.1 (default: the newest release,
#                         pre-releases included)
#   WARD_INSTALL_DIR      where `ward` goes (default: ~/.ward/bin)
#   WARD_NO_MODIFY_PATH   set to 1 to leave shell startup files alone
#   WARD_DOWNLOAD_BASE    serves <tag>/<archive> instead of GitHub releases (for tests)

set -eu

REPO="ahmedelraei/wardscript"
INSTALL_DIR="${WARD_INSTALL_DIR:-$HOME/.ward/bin}"

say() { printf '%s\n' "$*"; }
fail() { printf 'ward installer: %s\n' "$*" >&2; exit 1; }

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
    fetch_stdout() { curl -fsSL "$1"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -q "$1" -O "$2"; }
    fetch_stdout() { wget -q "$1" -O -; }
else
    fail "needs curl or wget"
fi

case "$(uname -s)" in
    Linux) os="unknown-linux-musl" ;;
    Darwin) os="apple-darwin" ;;
    *) fail "unsupported system $(uname -s); on Windows, use install.ps1" ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch="x86_64" ;;
    arm64 | aarch64) arch="aarch64" ;;
    *) fail "unsupported CPU $(uname -m)" ;;
esac
# A shell running under Rosetta reports x86_64 on Apple Silicon; install the native build.
if [ "$os" = "apple-darwin" ] && [ "$arch" = "x86_64" ] &&
    [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = "1" ]; then
    arch="aarch64"
fi
target="$arch-$os"

version="${WARD_VERSION:-}"
if [ -z "$version" ]; then
    [ -z "${WARD_DOWNLOAD_BASE:-}" ] || fail "WARD_DOWNLOAD_BASE needs WARD_VERSION"
    # /releases/latest skips pre-releases, and every release is one during the beta.
    version="$(fetch_stdout "https://api.github.com/repos/$REPO/releases?per_page=1" |
        sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
    [ -n "$version" ] || fail "couldn't find a release of $REPO"
fi
base="${WARD_DOWNLOAD_BASE:-https://github.com/$REPO/releases/download}/$version"
archive="ward-$version-$target.tar.gz"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading Wardscript $version for $target..."
fetch "$base/$archive" "$tmp/$archive" || fail "couldn't download $base/$archive"
fetch "$base/$archive.sha256" "$tmp/$archive.sha256" || fail "couldn't download the checksum"

expected="$(cut -d ' ' -f 1 <"$tmp/$archive.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$tmp/$archive" | cut -d ' ' -f 1)"
else
    actual="$(shasum -a 256 "$tmp/$archive" | cut -d ' ' -f 1)"
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch for $archive (expected $expected, got $actual)"

mkdir -p "$tmp/unpacked" "$INSTALL_DIR"
tar -xzf "$tmp/$archive" -C "$tmp/unpacked"
# Moving into place, not overwriting, lets a running `ward` (say, `ward lsp`) keep its file.
cp "$tmp/unpacked/ward" "$INSTALL_DIR/ward.new"
chmod 755 "$INSTALL_DIR/ward.new"
mv -f "$INSTALL_DIR/ward.new" "$INSTALL_DIR/ward"
say "Installed $("$INSTALL_DIR/ward" --version) to $INSTALL_DIR/ward"

add_path() {
    rc="$1"
    line="$2"
    if [ -f "$rc" ] && grep -qsF "$shown" "$rc"; then
        return
    fi
    mkdir -p "$(dirname "$rc")"
    printf '\n# Added by the Wardscript installer\n%s\n' "$line" >>"$rc"
    say "Added $INSTALL_DIR to PATH in $rc"
}

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        if [ "${WARD_NO_MODIFY_PATH:-0}" = "1" ]; then
            say "Add $INSTALL_DIR to your PATH to use \`ward\`."
        else
            # Written as $HOME/... when it's under home, so the line survives a moved home.
            case "$INSTALL_DIR" in
                "$HOME"/*) shown="\$HOME${INSTALL_DIR#"$HOME"}" ;;
                *) shown="$INSTALL_DIR" ;;
            esac
            export_line="export PATH=\"$shown:\$PATH\""
            add_path "$HOME/.profile" "$export_line"
            for rc in "$HOME/.bashrc" "$HOME/.zshrc"; do
                if [ -f "$rc" ]; then
                    add_path "$rc" "$export_line"
                fi
            done
            if [ -d "$HOME/.config/fish" ]; then
                add_path "$HOME/.config/fish/conf.d/ward.fish" "fish_add_path \"$shown\""
            fi
            say "Open a new terminal, or run: export PATH=\"$INSTALL_DIR:\$PATH\""
        fi
        ;;
esac

say ""
say "Wardscript is in beta: the language and its diagnostics may change between releases."
say "Get started: ward init hello && cd hello && ward check main.ward"
