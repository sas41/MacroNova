<p align="center">>
<img src="assets/logo-256.png" alt="MacroNova" width="96"/>
</p

# MacroNova

A macro daemon for Linux that lets you bind physical mouse and keyboard buttons
to [Rhai](https://rhai.rs/) scripts — run key sequences, type text, click,
scroll, warp the cursor, and more.

Currently targets **Logitech G502 X Lightspeed** (USB dongle, wireless) on
**Linux with KDE Plasma 6 / KWin Wayland**.  Windows and macOS stubs exist in
the codebase; contributions welcome.

---

## Features

- Read button events from a physical device without consuming them from the OS —
  normal pointer and key behaviour is completely unaffected
- Bind any button to a `.rhai` script via `on_press` and/or `on_release`
- Scripts run in a sandboxed Rhai engine with a clean, stable API
- Hold-aware: `held()` returns `false` when the button is released, so loops
  terminate cleanly between cycles
- Hot-reload: edits to `config.toml` take effect immediately without restarting
  the daemon
- GUI for managing bindings, capturing button names, and editing scripts live
- Virtual keyboard injection via `uinput` (works on Wayland)
- Absolute cursor warp via `uinput` `EV_ABS` (works on Wayland)
- Configurable warp mode: **Jitter** (default, most compatible) or **Direct**
  (`INPUT_PROP_DIRECT`)
- **Virtual Mode**: grab the physical device exclusively so the OS never sees
  the raw events; per-binding `intercept` flag suppresses forwarding of
  individual buttons while all other input is passed through transparently

---

## Requirements

- Linux kernel with `uinput` support (`/dev/uinput`)
- User in the `input` group, or udev rules granting access (see Installation)
- KDE Plasma 6 / KWin Wayland (X11 and other compositors untested)
- Rust 1.75+ (2021 edition)

---

## Building

```sh
cargo build --release
```

The workspace produces these binaries:

| Binary | Crate | Purpose |
|---|---|---|
| `macronova-daemon` | `macronova-daemon` | Background daemon (run as user) |
| `macronova-gui` | `macronova-gui` | Configuration GUI |
| `hidraw-sniffer` | `macronova-daemon` | Debug: print raw HID++ frames |
| `evdev-sniffer` | `macronova-daemon` | Debug: print raw evdev events |

---

## Installation

Use the release build script and installer:

```sh
publish/linux/build-release.sh
artifacts/linux-x64/install-macronova.sh
```

This installs binaries, the `.desktop` file, icons, bundled macros, and
optionally configures the systemd user service.  No `sudo` required — privileged
setup (udev rules, systemd) is handled by the **Daemon** tab in the GUI on first
launch.

### Manual udev rule

```sh
sudo cp publish/linux/42-macronova.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG input $USER   # re-login after this
```

### Manual daemon start

```sh
cargo run -p macronova-daemon
```

Or as a systemd user service:

```sh
cp publish/linux/macronova.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now macronova-daemon
```

---

## Configuration

Config lives at `~/.config/macronova/config.toml`.

```toml
# Wayland cursor warp mode: "jitter" (default) or "direct"
warp_mode = "jitter"

# Virtual Mode: grab the physical device exclusively (EVIOCGRAB).
# The OS never receives raw events; the daemon re-injects everything it does
# not intercept.  Per-binding intercept = true suppresses that button's
# passthrough so the compositor never sees it.
virtual_mode = false

[device.G502X]
wpid = "407F"

[[device.G502X.bindings]]
button    = "usb-Logitech_USB_Receiver-event-mouse/key0x0117"
on_press  = "macros/undo.rhai"
intercept = true   # suppress this button — OS never sees it

[[device.G502X.bindings]]
button     = "usb-Logitech_USB_Receiver-event-mouse/key0x0115"
on_press   = "macros/spam_lmb.rhai"
on_release = "macros/spam_lmb.rhai"
# intercept = false (default) — button still reaches the compositor
```

Use the GUI **Capture** button to discover the exact button name for any
physical key.  See [`SCRIPTING.md`](SCRIPTING.md) for the full API reference.

---

## Rhai Script API (quick reference)

```rhai
// Keyboard
press_key("ctrl")       // hold a key down
release_key("ctrl")     // release a held key
tap_key("z")            // press + release
type_text("hello")      // type a string character by character

// Mouse
click("left")           // click a mouse button
press_mouse("right")    // hold a mouse button down
release_mouse("right")  // release a mouse button
move_mouse(10, -5)      // relative cursor movement (pixels)
warp_mouse(960, 540)    // absolute cursor warp (logical pixels)
scroll(3)               // vertical scroll (positive = down)
hscroll(-1)             // horizontal scroll (positive = right)

// Control flow
sleep(50)               // sleep N milliseconds
held()                  // true while the trigger button is held

// System
run_command("notify-send MacroNova fired")
```

---

## Project layout

```
MacroNova/
  assets/                 # Logo files (PNG + SVG)
  crates/
    macronova-core/       # Config, evdev discovery, platform abstractions
    macronova-daemon/     # Main daemon binary + debug sniffers
    macronova-gui/        # egui configuration GUI
  publish/
    linux/                # Build + install scripts, .desktop, udev rules, service unit
    config/               # Example config and bundled macros
  Guides/                 # Device-specific setup guides
  ARCHITECTURE.md         # Crate structure, data flow, OS-specific implementation notes
  SCRIPTING.md            # Full Rhai API reference and key name list
```

---

## Guides

See the [`Guides/`](Guides/GUIDES.md) folder for device-specific setup and
example macro collections.
