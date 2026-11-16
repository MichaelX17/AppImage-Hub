#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|aarch64) ;;
  *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

BUILD_DIR="$ROOT/target/appimage"
APPDIR="$BUILD_DIR/AppDir"
TOOLS_DIR="$BUILD_DIR/tools"
DIST_DIR="$ROOT/dist"
OUTPUT="$DIST_DIR/AppImage-Hub-${VERSION}-${ARCH}.AppImage"

echo "==> Building release binary"
cargo build --release

echo "==> Preparing AppDir"
rm -rf "$APPDIR"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/icons/hicolor/512x512/apps" \
  "$APPDIR/usr/share/metainfo"

install -m 755 "$ROOT/target/release/appimage-manager" "$APPDIR/usr/bin/appimage-manager"
install -m 644 "$ROOT/assets/logo.png" "$APPDIR/appimage-hub.png"
install -m 644 "$ROOT/assets/logo.png" "$APPDIR/usr/share/icons/hicolor/512x512/apps/appimage-hub.png"
install -m 644 "$ROOT/packaging/io.github.michaelx17.AppImageHub.desktop" "$APPDIR/io.github.michaelx17.AppImageHub.desktop"
mkdir -p "$APPDIR/usr/share/applications"
install -m 644 "$ROOT/packaging/io.github.michaelx17.AppImageHub.desktop" "$APPDIR/usr/share/applications/io.github.michaelx17.AppImageHub.desktop"
install -m 644 "$ROOT/packaging/io.github.michaelx17.AppImageHub.metainfo.xml" \
  "$APPDIR/usr/share/metainfo/io.github.michaelx17.AppImageHub.appdata.xml"
install -m 644 "$ROOT/packaging/.appimage-hub.json" "$APPDIR/.appimage-hub.json"

ln -sf appimage-hub.png "$APPDIR/.DirIcon"

cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/appimage-manager" "$@"
APPRUN
chmod 755 "$APPDIR/AppRun"

mkdir -p "$DIST_DIR" "$TOOLS_DIR"
APPIMAGETOOL="${APPIMAGETOOL:-}"
if [ -z "$APPIMAGETOOL" ] && command -v appimagetool >/dev/null 2>&1; then
  APPIMAGETOOL="$(command -v appimagetool)"
fi
if [ -z "$APPIMAGETOOL" ]; then
  APPIMAGETOOL="$TOOLS_DIR/appimagetool-${ARCH}.AppImage"
  if [ ! -x "$APPIMAGETOOL" ]; then
    echo "==> Downloading appimagetool"
    curl -fL --retry 3 \
      "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${ARCH}.AppImage" \
      -o "$APPIMAGETOOL"
    chmod +x "$APPIMAGETOOL"
  fi
fi

echo "==> Creating $OUTPUT"
ARCH="$ARCH" "$APPIMAGETOOL" --appimage-extract-and-run "$APPDIR" "$OUTPUT"
chmod +x "$OUTPUT"

echo "==> Built: $OUTPUT"
sha256sum "$OUTPUT"
