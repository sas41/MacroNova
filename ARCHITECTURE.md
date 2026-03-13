# Architecture

## Crate overview

```
macronova-core      (library)
  ├─ config.rs              Config / DeviceConfig / ButtonBinding / WarpMode
  │                         load/save, path helpers, legacy name migration
  ├─ platform/
  │    └─ input.rs          InputInjector trait, get_desktop_size(),
  │                         get_cursor_position() — platform-dispatched
  └─ device/
       ├─ evdev_input.rs    Raw evdev reader — button event sniffing (Linux)
       ├─ hidraw_input.rs   HID++ hidraw reader (research / debug)
       ├─ logitech/         Logitech-specific constants and device matching
       └─ hidpp/            HID++ protocol framing helpers

macronova-daemon    (binary: macronova-daemon)
  ├─ main.rs                Entry point, event loop, config hot-reload
  ├─ engine/
  │    └─ rhai.rs           Sandboxed Rhai engine, ScriptContext, run_script()
  └─ input/
       ├─ hidpp_reader.rs   HID++ notification reader (CID-based bindings)
       └─ platform/
            ├─ mod.rs       Re-exports PlatformInjector for the current OS
            ├─ linux/
            │    └─ injector.rs   uinput keyboard + mouse (EV_REL + EV_ABS)
            ├─ windows/
            │    └─ injector.rs   Stub (SendInput — not yet implemented)
            └─ macos/
                 └─ injector.rs   Stub (CGEventPost — not yet implemented)

macronova-daemon    (binaries: hidraw-sniffer, evdev-sniffer)
  └─ bin/
       ├─ hidraw_sniffer.rs   Print raw HID++ frames from /dev/hidraw*
       └─ evdev_sniffer.rs    Print raw evdev events from /dev/input/event*

macronova-gui       (binary: macronova-gui)
  ├─ app.rs             MacroNovaApp — top-level egui app, owns Config
  └─ views/
       ├─ bindings.rs   Button → script binding editor, capture mode
       ├─ editor.rs     Inline .rhai script text editor
       ├─ devices.rs    Device info panel
       ├─ daemon.rs     Daemon status / systemd control panel
       └─ settings.rs   Global settings (warp mode, etc.)
```

---

## Platform support matrix

| Feature | Linux | Windows | macOS |
|---|---|---|---|
| Button sniffing | evdev (`/dev/input/event*`) | _stub_ | _stub_ |
| Keyboard injection | uinput `EV_KEY` | _stub_ (SendInput) | _stub_ (CGEventPost) |
| Mouse click / relative motion | uinput `EV_REL` | _stub_ | _stub_ |
| Absolute cursor warp | uinput `EV_ABS` | _stub_ | _stub_ |
| Desktop size query | Wayland `wl_output` / X11 `XDisplayWidth` | _stub_ | _stub_ |
| Cursor position query | Not supported on Wayland; `XQueryPointer` on X11 | _stub_ | _stub_ |

---

## Data flow

### Input sniffing (Linux)

```
/dev/input/by-id/*-event-mouse   (evdev node, O_RDONLY — no EVIOCGRAB)
/dev/input/by-id/*-if01-event-kbd
        │
        │  libc::read → input_event { type=EV_KEY, code, value }
        ▼
  EvdevReader::poll()
        │
        │  ButtonEvent { button: ButtonId, pressed: bool }
        ▼
  main.rs event loop
        │
        ├─ pressed=true  → handle_button_down()
        │                      look up on_press script in Config
        │                      spawn macro thread via run_script()
        │
        └─ pressed=false → handle_button_up()
                               set held AtomicBool = false
                               look up on_release script, run if present
```

The evdev fd is opened with plain `O_RDONLY` — **no `EVIOCGRAB`**. The kernel
delivers a copy of every event to each open fd independently, so the daemon reads
events without consuming them from the OS input stack.

Two evdev nodes are opened for the Logitech USB receiver:

| By-id symlink suffix | Contents |
|---|---|
| `usb-Logitech_USB_Receiver-event-mouse` | Mouse buttons + movement |
| `usb-Logitech_USB_Receiver-if01-event-kbd` | DPI cycle / consumer keys |

Both are discovered automatically via `/dev/input/by-id/` at startup with
automatic reconnect on unplug/replug.

### Keyboard injection

#### Linux

```
Rhai script calls press_key / tap_key / type_text
        │
        ▼
  uinput virtual device "macronova-kbd"  (BUS_USB, EV_KEY codes 1–255)
        │
        │  kernel assigns "kbd" handler → /dev/input/eventN
        ▼
  XWayland reads "kbd" handler directly (bypasses libinput)
        │
        ▼
  Focused X11 or Wayland-native window receives key event
```

Setting `BUS_USB` in the uinput descriptor causes the kernel to assign the `kbd`
handler to the virtual device. XWayland reads `kbd` handler nodes directly,
bypassing libinput, so key events reach both X11 and Wayland-native windows.

#### Windows (stub)

Planned: `SendInput()` with `INPUT_KEYBOARD` / `KEYBDINPUT` structs.
See `crates/macronova-daemon/src/input/platform/windows/injector.rs`.

#### macOS (stub)

Planned: `CGEventCreateKeyboardEvent()` + `CGEventPost(kCGHIDEventTap, ...)`.
See `crates/macronova-daemon/src/input/platform/macos/injector.rs`.

### Mouse injection

#### Linux

```
Rhai script calls click / move_mouse / warp_mouse / scroll …
        │
        ▼
  UInputInjector::click / move_rel / warp / scroll
        │
        ▼
  uinput virtual device "macronova-mouse"
        │  BUS_USB
        │  EV_KEY  BTN_LEFT … BTN_EXTRA
        │  EV_REL  REL_X, REL_Y, REL_WHEEL, REL_HWHEEL
        │  EV_ABS  ABS_X, ABS_Y  [0, 32767]
        ▼
  compositor maps [0, 32767] → full logical desktop (warp)
  compositor applies relative deltas (move_mouse / scroll)
```

`warp_mouse(x, y)` scales logical pixel coordinates to the `[0, 32767]` absolute
axis range and emits `EV_ABS` events. Because the kernel deduplicates `EV_ABS`
events with the same value, repeated warps to the same position need a
workaround; two modes are supported (configurable in Settings):

| Mode | Behaviour |
|---|---|
| `jitter` (default) | Emits `(x, y±1)` then `(x, y)` — forces a state change every call; works on all compositors |
| `direct` | Declares `INPUT_PROP_DIRECT` (tablet/touchscreen semantics) on the uinput device — the compositor applies every event unconditionally |

The desktop size used for warp scaling is queried at daemon startup:
- **Wayland**: iterates `wl_output` geometry/scale/mode events to compute the
  bounding box of all logical outputs
- **X11**: `XDisplayWidth` / `XDisplayHeight`

#### Windows (stub)

Planned: `SendInput()` with `INPUT_MOUSE` / `MOUSEINPUT` structs.
`warp_mouse` will use `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE` with
coordinates normalised to `[0, 65535]`.

#### macOS (stub)

Planned: `CGEventCreateMouseEvent()` + `CGEventPost(kCGHIDEventTap, ...)`.
`warp_mouse` will use `CGEventSetLocation()`.

### Script execution

```
run_script(path, held: Arc<AtomicBool>, injector: Arc<Mutex<dyn InputInjector>>)
        │
        │  read script source from disk
        │  build Rhai Engine with ScriptContext registered
        │  spawn std::thread  ("macro:<path>")
        ▼
  Rhai Engine::eval()
        │
        └─ registered functions call into injector via Arc<Mutex<>>
           held() checks the AtomicBool directly
```

Each script runs in its own thread. The `held` `AtomicBool` is shared between
the event loop thread (sets it `false` on button release) and the script thread
(reads it via `held()`). Scripts are never forcibly interrupted — termination
happens only at `while held()` loop boundaries, ensuring press/release pairs are
always completed.

### Config hot-reload

```
notify::RecommendedWatcher watches ~/.config/macronova/
        │
        │  inotify Modify/Create events on config.toml
        ▼
  Config::load() → replace Arc<Mutex<Config>>
```

The config `Arc<Mutex<Config>>` is re-locked on every button event to look up
bindings, so a newly loaded config takes effect on the very next button press.

---

## Threading model

| Thread | Role |
|---|---|
| Main thread | evdev poll loop + config watcher drain |
| `macro:<path>` threads | One per active script invocation; terminate when script ends or `held()` returns false |

`UInputInjector` is wrapped in `Arc<Mutex<dyn InputInjector>>` and shared
across macro threads. The Rhai engine is constructed fresh for each script
invocation (no shared engine state between scripts).

---

## Config format

`~/.config/macronova/config.toml`

```toml
# Global settings
warp_mode = "jitter"   # "jitter" (default) | "direct"

[device.<name>]
wpid    = "407F"       # Logitech wireless product ID (optional, informational)
usb_pid = "C099"       # USB product ID (optional, informational)

[[device.<name>.bindings]]
button     = "usb-Logitech_USB_Receiver-event-mouse/key0x0115"
on_press   = "macros/foo.rhai"    # relative to ~/.config/macronova/
on_release = "macros/bar.rhai"    # optional
```

Button names use a stable by-id label format:
`<by-id-symlink-name>/key0x<HHHH>` where `HHHH` is the four-digit lowercase
`EV_KEY` hex code. Legacy `eventN/key0x...` names are automatically migrated on
load.

---

## Key technical findings (Linux / Wayland)

### Why uinput `EV_ABS` works for absolute warping on Wayland

The compositor maps the declared `[min, max]` range of an absolute axis linearly
to the full logical desktop. Declaring `ABS_X` and `ABS_Y` with range
`[0, 32767]` on the uinput mouse device lets the daemon warp the cursor to any
logical position without querying the current cursor location — which is
impossible on Wayland without a privileged protocol.

### Why cursor position is unavailable on Wayland

Wayland deliberately does not expose the global cursor position to unprivileged
clients. `get_cursor_position()` returns `None` and logs a warning on Wayland.
On X11 it uses `XQueryPointer`.

### Why `EV_ABS` deduplication requires a workaround

The Linux kernel tracks the last-reported value for each absolute axis and
silently drops events whose value has not changed. Repeated `warp_mouse()` calls
to the same position would be ignored after the first. The `jitter` mode works
around this by emitting a one-pixel offset event before the real target.

### Why the virtual keyboard works on Wayland

XWayland opens and reads `kbd`-handler evdev nodes directly, bypassing libinput.
Setting `BUS_USB` in the uinput descriptor causes the kernel to assign the `kbd`
handler to the virtual device. Key events sent to it are delivered to whichever
window XWayland has focused, including native Wayland windows via XWayland's
Wayland surface.

### Why a combined keyboard+mouse uinput device was split into two

A uinput device with both `KEY_*` and `EV_REL` capabilities is classified by
libinput as an ambiguous "keyboard+pointer" profile. On some compositors this
causes relative motion events to be ignored or misrouted. Splitting into a
dedicated keyboard device (`macronova-kbd`) and a dedicated mouse device
(`macronova-mouse`) ensures each is classified and routed correctly.

### Why KDE Plasma shows the correct window icon

On Wayland, compositors resolve window icons via the `app_id` set on the
`xdg_toplevel` surface, not from data embedded in the application. eframe
exposes this as `ViewportBuilder::with_app_id()`. Setting
`with_app_id("macronova-gui")` causes KDE to match the window against
`macronova-gui.desktop` and use its `Icon=macronova` field, which resolves to
`~/.local/share/icons/hicolor/256x256/apps/macronova.png` (installed by the
installer). `with_icon()` / `IconData` is used as a fallback for platforms that
support embedded icons (Windows task bar, etc.).
