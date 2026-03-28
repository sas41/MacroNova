<p align="center">
  <img src="assets/logo-256.png" alt="MacroNova" width="96"/>
</p>

# MacroNova

A Linux macro daemon that binds physical input events to [Rhai](https://rhai.rs/) scripts.

MacroNova is now device-first: you add real evdev devices in the GUI, then add bindings under each selected device. Capture is scoped per device, so two devices can use the same event code without collisions. The daemon runtime is evdev-only for input event ingestion.

---

## Features

- Explicit device configuration from `/dev/input/by-id` candidates
- Per-device capture scope (`Capture` on device A ignores events from device B)
- Per-device button bindings with `on_press` / `on_release` script hooks
- Virtual Mode (`EVIOCGRAB`) with per-binding `intercept`
- Hot-reload of `config.toml`
- Rhai scripting for key/mouse injection, loops, delays, and shell commands
- Wayland-compatible keyboard/mouse injection via `uinput`

---

## Requirements

- Linux kernel with `uinput` support (`/dev/uinput`)
- User in `input` group, or equivalent udev permissions
- Rust 1.75+

---

## Build

```sh
cargo build --release
```

Main binaries:

| Binary | Purpose |
|---|---|
| `macronova-daemon` | Background daemon |
| `macronova-gui` | GUI config/editor |
| `evdev-sniffer` | Raw evdev debug |
| `hidraw-sniffer` | Raw HID++ debug |

---

## Installation

```sh
publish/linux/build-release.sh
artifacts/linux-x64/install-macronova.sh
```

Optional manual udev setup:

```sh
sudo cp publish/linux/42-macronova.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG input $USER
```

---

## Quick Start (New Flow)

1. Launch `macronova-gui`.
2. Go to **Bindings**.
3. Click `+ Add device` and pick your device from the list.
4. Under that device, click `+ Add binding`.
5. Click `Capture` on that binding and press the physical button on that same device.
6. Choose a script for `on_press`.
7. Start daemon: `cargo run -p macronova-daemon`.

---

## Configuration

Config path: `~/.config/macronova/config.toml`

```toml
warp_mode = "jitter"
virtual_mode = false

[[devices]]
id = "usb-Logitech_USB_Receiver"
display_name = "Logitech USB Receiver"
mouse_path = "/dev/input/by-id/usb-Logitech_USB_Receiver-event-mouse"
kbd_path = "/dev/input/by-id/usb-Logitech_USB_Receiver-if01-event-kbd"

[[devices.bindings]]
button = "usb-Logitech_USB_Receiver::usb-Logitech_USB_Receiver-event-mouse/key0x0113"
on_press = "macros/back.rhai"
intercept = true

[[devices.bindings]]
button = "usb-Logitech_USB_Receiver::usb-Logitech_USB_Receiver-event-mouse/key0x0114"
on_press = "macros/forward.rhai"
on_release = "macros/forward_release.rhai"
intercept = false
```

Notes:
- `devices` is a list; each entry is one physical device source.
- `button` is device-scoped with the `device_id::...` prefix.
- Device paths should remain `/dev/input/by-id/...` symlinks so event number
  renumbering (`eventN`) is handled automatically.
- This format is a breaking change from older `device.<name>` configs.

---

## Guides

See [`Guides/GUIDES.md`](Guides/GUIDES.md) for device-specific setup.
