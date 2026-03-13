#!/usr/bin/env bash
# MacroNova release build script.
# Compiles release binaries and assembles the artifacts/linux-x64/ distribution tree.
# Run from anywhere — the script locates the repo root automatically.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PUBLISH_DIR="$(dirname "$SCRIPT_DIR")"
REPO_DIR="$(dirname "$PUBLISH_DIR")"
OUT_DIR="$REPO_DIR/artifacts/linux-x64"
PKG_DIR="$OUT_DIR/MacroNova"

echo "Building release binaries..."
cargo build --release \
    --bin macronova-daemon \
    --bin macronova-gui \
    --manifest-path "$REPO_DIR/Cargo.toml"

echo "Assembling distribution tree at $PKG_DIR ..."

# ── Clean previous build ─────────────────────────────────────────────────────
rm -rf "$OUT_DIR"

# ── Create directory tree ────────────────────────────────────────────────────
mkdir -p "$PKG_DIR/MacroNova-Daemon/macros"
mkdir -p "$PKG_DIR/MacroNova-GUI/icons"

# ── Daemon ───────────────────────────────────────────────────────────────────
cp "$REPO_DIR/target/release/macronova-daemon"  "$PKG_DIR/MacroNova-Daemon/macronova-daemon"
cp "$SCRIPT_DIR/42-macronova.rules"             "$PKG_DIR/MacroNova-Daemon/42-macronova.rules"
cp "$SCRIPT_DIR/macronova.service"              "$PKG_DIR/MacroNova-Daemon/macronova.service"
cp "$PUBLISH_DIR/config/macros/"*.rhai          "$PKG_DIR/MacroNova-Daemon/macros/"

# ── GUI ──────────────────────────────────────────────────────────────────────
cp "$REPO_DIR/target/release/macronova-gui"     "$PKG_DIR/MacroNova-GUI/macronova-gui"
cp "$SCRIPT_DIR/macronova-gui.desktop"          "$PKG_DIR/MacroNova-GUI/macronova-gui.desktop"
cp "$REPO_DIR/assets/logo-256.png"              "$PKG_DIR/MacroNova-GUI/icons/macronova-256.png"
cp "$REPO_DIR/assets/logo.svg"                  "$PKG_DIR/MacroNova-GUI/icons/macronova.svg"

# ── Install script ───────────────────────────────────────────────────────────
cp "$SCRIPT_DIR/install-macronova.sh"           "$OUT_DIR/install-macronova.sh"
chmod +x "$OUT_DIR/install-macronova.sh"
chmod +x "$PKG_DIR/MacroNova-Daemon/macronova-daemon"
chmod +x "$PKG_DIR/MacroNova-GUI/macronova-gui"

echo ""
echo "Done. Distribution tree:"
find "$OUT_DIR" | sort | sed "s|$REPO_DIR/||"
