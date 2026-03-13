#!/usr/bin/env bash
# MacroNova installer / updater
# Safe to run on first install and on every subsequent update.
# Must be run from the assembled artifacts/linux-x64/ directory:
#   artifacts/linux-x64/install-macronova.sh
# Build first with: publish/linux/build-release.sh
# Does NOT require sudo — privileged setup (udev rules, systemd service) is
# handled by the MacroNova GUI on first launch.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKG_DIR="$SCRIPT_DIR/MacroNova"

if [ ! -d "$PKG_DIR/MacroNova-Daemon" ]; then
    echo "Error: MacroNova/ folder not found next to this script."
    echo "Run publish/linux/build-release.sh first, then:"
    echo "  artifacts/linux-x64/install-macronova.sh"
    exit 1
fi

LIB_ROOT="$HOME/.local/lib/MacroNova"
BIN_DIR="$HOME/.local/bin"
APPLICATIONS_DIR="$HOME/.local/share/applications"
ICONS_256_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
ICONS_SVG_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"
MACROS_DEST="$HOME/.config/macronova/macros"

# ── Stop daemon if running ───────────────────────────────────────────────────
DAEMON_WAS_RUNNING=false
if systemctl --user is-active --quiet macronova-daemon 2>/dev/null; then
    echo "Stopping macronova-daemon ..."
    systemctl --user stop macronova-daemon
    DAEMON_WAS_RUNNING=true
fi

# ── Wipe and recreate lib directories ───────────────────────────────────────
echo "Installing MacroNova to $LIB_ROOT ..."
rm -rf "$LIB_ROOT"
mkdir -p "$LIB_ROOT/MacroNova-Daemon"
mkdir -p "$LIB_ROOT/MacroNova-GUI"
mkdir -p "$BIN_DIR"
mkdir -p "$APPLICATIONS_DIR"
mkdir -p "$ICONS_256_DIR"
mkdir -p "$ICONS_SVG_DIR"

# ── Daemon ───────────────────────────────────────────────────────────────────
cp "$PKG_DIR/MacroNova-Daemon/macronova-daemon"   "$LIB_ROOT/MacroNova-Daemon/macronova-daemon"
cp "$PKG_DIR/MacroNova-Daemon/42-macronova.rules" "$LIB_ROOT/MacroNova-Daemon/42-macronova.rules"
chmod +x "$LIB_ROOT/MacroNova-Daemon/macronova-daemon"

# ── GUI ──────────────────────────────────────────────────────────────────────
cp "$PKG_DIR/MacroNova-GUI/macronova-gui" "$LIB_ROOT/MacroNova-GUI/macronova-gui"
chmod +x "$LIB_ROOT/MacroNova-GUI/macronova-gui"

# ── Icons ────────────────────────────────────────────────────────────────────
cp "$PKG_DIR/MacroNova-GUI/icons/macronova-256.png" "$ICONS_256_DIR/macronova.png"
cp "$PKG_DIR/MacroNova-GUI/icons/macronova.svg"     "$ICONS_SVG_DIR/macronova.svg"
# Refresh icon caches so desktop environments pick up the new icon immediately.
# GTK / most desktops
if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
fi
# KDE Plasma 6
if command -v kbuildsycoca6 &>/dev/null; then
    kbuildsycoca6 2>/dev/null || true
# KDE Plasma 5 fallback
elif command -v kbuildsycoca5 &>/dev/null; then
    kbuildsycoca5 2>/dev/null || true
fi
echo "Installed icons to $ICONS_256_DIR and $ICONS_SVG_DIR"

# ── Symlinks ─────────────────────────────────────────────────────────────────
ln -sf "$LIB_ROOT/MacroNova-Daemon/macronova-daemon" "$BIN_DIR/macronova-daemon"
ln -sf "$LIB_ROOT/MacroNova-GUI/macronova-gui"       "$BIN_DIR/macronova-gui"
echo "Linked executables in $BIN_DIR"

# ── .desktop file ────────────────────────────────────────────────────────────
# Only patch the Exec= path; Icon= uses the XDG name "macronova" which the
# desktop environment resolves from the hicolor icon theme automatically.
sed \
    -e "s|Exec=macronova-gui|Exec=$BIN_DIR/macronova-gui|g" \
    "$PKG_DIR/MacroNova-GUI/macronova-gui.desktop" \
    > "$APPLICATIONS_DIR/macronova-gui.desktop"
chmod +x "$APPLICATIONS_DIR/macronova-gui.desktop"
echo "Installed .desktop file to $APPLICATIONS_DIR"

# ── Example macros ───────────────────────────────────────────────────────────
# Always sync bundled macros into a subfolder the user can reference,
# but never touch macros the user has created or edited.
BUNDLED_MACROS_DEST="$MACROS_DEST/bundled"
mkdir -p "$BUNDLED_MACROS_DEST"
rm -rf "$BUNDLED_MACROS_DEST"
cp -r "$PKG_DIR/MacroNova-Daemon/macros" "$BUNDLED_MACROS_DEST"
echo "Updated bundled macros in $BUNDLED_MACROS_DEST"

# Seed the top-level macros directory only if no .rhai files exist there yet.
if ! ls "$MACROS_DEST/"*.rhai &>/dev/null; then
    cp "$PKG_DIR/MacroNova-Daemon/macros/"*.rhai "$MACROS_DEST/"
    echo "Seeded $MACROS_DEST with example macros"
fi

# ── Reload systemd and restart if service file exists ───────────────────────
SERVICE_FILE="$HOME/.config/systemd/user/macronova-daemon.service"
if [ -f "$SERVICE_FILE" ]; then
    # Overwrite with the latest service file from the package.
    cp "$PKG_DIR/MacroNova-Daemon/macronova.service" "$SERVICE_FILE"
    systemctl --user daemon-reload
    echo "Updated systemd service file"
    if [ "$DAEMON_WAS_RUNNING" = true ]; then
        echo "Restarting macronova-daemon ..."
        systemctl --user start macronova-daemon
    fi
else
    echo "Systemd service not yet installed — open the Daemon tab in the GUI to set it up."
fi

# ── PATH reminder ────────────────────────────────────────────────────────────
if ! echo "$PATH" | grep -q "$BIN_DIR"; then
    echo ""
    echo "NOTE: $BIN_DIR is not in your PATH."
    echo "Add the following line to your ~/.bashrc or ~/.profile:"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

echo ""
echo "Installation complete."
echo "Launch MacroNova from your application menu or run: macronova-gui"
