# MacroNova

A universal macro daemon for Linux. Bind physical mouse and keyboard buttons to
Rhai scripts — run key sequences, type text, click, scroll, and more.

Currently targets **Logitech G502 X Lightspeed** (USB dongle, wireless) on
**Linux with KDE Plasma Wayland / KWin**.

---

## Features

- Sniff input from a physical device without interfering with normal OS input
- Bind any button to a `.rhai` script via `on_press` and/or `on_release`
- Scripts run in a sandboxed Rhai engine with a defined API
- Hold-aware: `held()` returns false when the button is released, so loops
  terminate cleanly
- Hot-reload config: edits to `config.toml` take effect immediately without
  restarting the daemon
- GUI for managing bindings, capturing button names, and editing scripts
- Virtual keyboard injection via uinput (works on Wayland through XWayland's
  `kbd` handler)
- Mouse injection via the XDG RemoteDesktop portal + EIS (KDE prompts once for
  permission; subsequent starts reuse the grant automatically)

---

## Requirements

- Linux kernel with `uinput` support (`/dev/uinput`)
- User must be in the `input` group (for reading evdev nodes)
- KDE Plasma 6 / KWin Wayland (X11 and other compositors untested)
- Rust 1.75+ (uses the 2021 edition)

---

## Building

```sh
cargo build --release
```

The workspace produces three binaries:

| Binary | Crate | Purpose |
|---|---|---|
| `macronova-daemon` | `macronova-daemon` | Background daemon (run as user) |
| `macronova-gui` | `macronova-gui` | Configuration GUI |
| `hidraw-sniffer` | `macronova-daemon` | Debug: print raw HID++ frames |
| `evdev-sniffer` | `macronova-daemon` | Debug: print raw evdev events |
| `mouse-test` | `macronova-daemon` | Debug: test EIS mouse click injection |

---

## Installation

### udev rule (required)

The evdev nodes for the receiver are owned by `root:input`. Add yourself to the
`input` group and install the udev rule so that future plugs are accessible
without sudo:

```sh
sudo cp 42-macronova.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG input $USER   # re-login after this
```

### Daemon

Run manually:

```sh
cargo run -p macronova-daemon
```

Or install as a systemd user service:

```sh
cp macronova.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now macronova
```

The service file expects the binary at `~/.cargo/bin/macronova-daemon`:

```sh
cargo install --path crates/macronova-daemon
```

---

## Configuration

Config lives at `~/.config/macronova/config.toml`. See
`config/example.toml` for a fully commented example.

```toml
[device.G502X]
wpid = "407F"

[[device.G502X.bindings]]
button = "event5/key0x0117"   # sniper button
on_press = "macros/undo.rhai"

[[device.G502X.bindings]]
button = "event5/key0x0115"   # top side button 1
on_press  = "macros/click_repeat.rhai"
on_release = "macros/notify_released.rhai"
```

Button names use the format `<eventN>/key0x<HHHH>` where `HHHH` is the
four-digit lowercase EV_KEY hex code. Use the GUI **Capture** button to
discover the exact name for any physical button.

### Script directory

Scripts are `.rhai` files placed in `~/.config/macronova/macros/`. Paths in
the config are relative to `~/.config/macronova/`.

---

## Rhai Script API

```rhai
// Keyboard
press_key("ctrl")       // hold a key down
release_key("ctrl")     // release a held key
tap_key("z")            // press + release
type_text("hello")      // type a string

// Mouse (via XDG RemoteDesktop portal + EIS — KDE will prompt once for permission)
click("left")           // click a mouse button
press_mouse("right")    // hold a mouse button down
release_mouse("right")  // release a mouse button
move_mouse(10, -5)      // relative cursor movement
warp_mouse(960, 540)    // absolute cursor warp
scroll(3)               // vertical scroll (positive = down)
hscroll(-1)             // horizontal scroll (positive = right)

// Control
sleep(50)               // sleep N milliseconds
held()                  // true while the trigger button is held

run_command("notify-send MacroNova fired")
```

`held()` is checked at `while` loop boundaries. Loops terminate cleanly after
the current cycle completes — press/release pairs are never split mid-cycle.
See **SCRIPTING.md** for the full API reference and key name list.

---

## Mouse Injection

Mouse injection uses the **XDG RemoteDesktop portal** (`ConnectToEIS`) backed
by `xdg-desktop-portal-kde`. On first run KDE shows a one-time permission
dialog; the grant is persisted so subsequent daemon starts need no interaction.
See **ARCHITECTURE.md** for implementation details and research history.

---

## Project layout

```
MacroNova/
  crates/
    macronova-core/     # config, evdev discovery, device abstractions
    macronova-daemon/   # main daemon binary + debug sniffers
    macronova-gui/      # egui configuration GUI
  config/
    example.toml        # annotated example configuration
  macros/               # example .rhai scripts
  42-macronova.rules    # udev rule for /dev/input access
  macronova.service     # systemd user service unit
  ARCHITECTURE.md       # crate and data-flow documentation
  SCRIPTING.md          # Rhai API reference and key name list
```
