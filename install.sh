#!/bin/sh
# Install Templum on Linux or macOS:
#
#   curl -fsSL https://github.com/KingDasWinx/Templum/releases/latest/download/install.sh | sh
#
# Linux: the executable goes to ~/.local/bin, with an entry in your apps menu. macOS: Templum.app goes to
# /Applications (~/Applications when that isn't yours to write). Templum updates itself from there.
#
#   TEMPLUM_VERSION=0.1.2       that version instead of the latest: curl … | TEMPLUM_VERSION=0.1.2 sh
#   TEMPLUM_INSTALL_DIR=<dir>   that folder instead
#
# On Arch Linux, the AUR has it too: yay -S templum-bin (or paru -S templum-bin).
#
# release.yml attaches this file to every release.
set -eu

repo=https://github.com/KingDasWinx/Templum

say() { printf 'templum: %s\n' "$*"; }
die() {
  printf 'templum: %s\n' "$*" >&2
  exit 1
}

command -v curl >/dev/null || die "curl is needed"
if command -v sha256sum >/dev/null; then sha256=sha256sum; else sha256='shasum -a 256'; fi

version=${TEMPLUM_VERSION:-}
if [ -z "$version" ]; then
  # releases/latest redirects to releases/tag/v<version>.
  latest=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$repo/releases/latest") || die "couldn't reach GitHub"
  version=${latest##*/}
fi
version=${version#v}
case $version in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *) die "not a version: $version" ;;
esac

tmp=$(mktemp -d)
mounted=
cleanup() {
  [ -z "$mounted" ] || hdiutil detach -quiet "$mounted" || true
  rm -rf "$tmp"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Downloads a file of the release into $tmp, and checks it against the .sha256 beside it.
fetch() {
  say "downloading $1"
  curl -fL --progress-bar -o "$tmp/$1" "$repo/releases/download/v$version/$1" ||
    die "couldn't download $1 of Templum $version ($repo/releases)"
  curl -fsSL -o "$tmp/$1.sha256" "$repo/releases/download/v$version/$1.sha256" || die "couldn't download $1.sha256"
  # $sha256 is two words on macOS.
  # shellcheck disable=SC2086
  [ "$(cut -d ' ' -f 1 <"$tmp/$1.sha256")" = "$($sha256 "$tmp/$1" | cut -d ' ' -f 1)" ] ||
    die "$1 doesn't match its checksum; nothing was installed"
}

case $(uname -s) in
  Linux)
    case $(uname -m) in
      x86_64 | amd64) target=x86_64-unknown-linux-gnu ;;
      aarch64 | arm64) target=aarch64-unknown-linux-gnu ;;
      *) die "no Linux build for $(uname -m): see $repo/blob/main/CONTRIBUTING.md to build it" ;;
    esac
    fetch "templum-$target.tar.gz"
    tar -xzf "$tmp/templum-$target.tar.gz" -C "$tmp"
    bin=${TEMPLUM_INSTALL_DIR:-$HOME/.local/bin}
    mkdir -p "$bin"
    # Copied beside, then renamed over: a Templum running from there keeps running.
    cp "$tmp/templum-$target/templum" "$bin/.templum.install"
    chmod 755 "$bin/.templum.install"
    mv -f "$bin/.templum.install" "$bin/templum"

    data=${XDG_DATA_HOME:-$HOME/.local/share}
    icons=$data/icons/hicolor/256x256/apps
    mkdir -p "$data/applications" "$icons"
    curl -fsSL -o "$icons/templum.png" \
      "https://raw.githubusercontent.com/KingDasWinx/Templum/v$version/crates/templum/icons/256x256.png" ||
      say "couldn't download the icon: the apps menu shows a plain one"
    cat >"$data/applications/templum.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Templum
Comment=An infinite canvas of terminals and AI agents
Exec="$bin/templum"
Icon=templum
Terminal=false
Categories=Development;
StartupWMClass=templum
EOF
    case ":$PATH:" in
      *":$bin:"*) say "installed Templum $version: open it from your apps menu, or run templum" ;;
      *) say "installed Templum $version in $bin: open it from your apps menu (add $bin to PATH to run templum)" ;;
    esac
    ;;
  Darwin)
    case $(uname -m) in
      arm64) arch=aarch64 ;;
      x86_64) arch=x64 ;;
      *) die "no macOS build for $(uname -m)" ;;
    esac
    fetch "Templum_${version}_$arch.dmg"
    apps=${TEMPLUM_INSTALL_DIR:-/Applications}
    [ -n "${TEMPLUM_INSTALL_DIR:-}" ] || [ -w /Applications ] || apps=$HOME/Applications
    mkdir -p "$apps"
    hdiutil attach -nobrowse -readonly -noautoopen -quiet -mountpoint "$tmp/dmg" "$tmp/Templum_${version}_$arch.dmg"
    mounted=$tmp/dmg
    # curl doesn't quarantine what it downloads, so macOS opens this app without asking.
    rm -rf "$apps/Templum.app"
    ditto "$mounted/Templum.app" "$apps/Templum.app"
    say "installed Templum $version in $apps: open it from Launchpad or Spotlight"
    ;;
  *)
    die "on Windows, run this in PowerShell: irm $repo/releases/latest/download/install.ps1 | iex"
    ;;
esac
