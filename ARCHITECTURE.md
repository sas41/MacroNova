# Architecture

## Crate overview

```
macronova-core      (library)
  ├─ config.rs          Config / DeviceConfig / ButtonBinding, load/save, path helpers
  └─ device/
       ├─ evdev_input.rs    Raw evdev reader — button event sniffing
       ├─ hidraw_input.rs   HID++ hidraw reader (unused in daemon, kept for research)
       ├─ logitech/         Logitech-specific constants and device matching
       └─ hidpp/            HID++ protocol framing helpers

macronova-daemon    (binary: macronova-daemon)
  ├─ main.rs            Entry point, event loop, config hot-reload
  ├─ input/
  │    ├─ uinput.rs     Virtual keyboard via uinput (BUS_USB trick for XWayland)
  │    └─ xtest.rs      Mouse injector stub (see MOUSE.md)
  └─ engine/
       └─ rhai.rs       Sandboxed Rhai engine, ScriptContext, run_script()

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
       └─ daemon.rs     Daemon status panel
```

---

## Data flow

### Input sniffing (daemon)

```
/dev/input/eventN  (evdev node, opened O_RDONLY|O_NONBLOCK)
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
                               set held AtomicBool = false  (stops running script)
                               look up on_release script, run if present
```

The evdev fd is opened with plain `O_RDONLY` — **no `EVIOCGRAB`**. The kernel
delivers a copy of every event to each open fd independently, so the daemon
reads events without consuming them from the OS input stack. Normal pointer /
key behaviour is completely unaffected.

Two evdev nodes are opened for the Logitech USB receiver:

| Node (by-id symlink suffix) | Contents |
|---|---|
| `usb-Logitech_USB_Receiver-event-mouse` | Mouse buttons + movement |
| `usb-Logitech_USB_Receiver-if01-event-kbd` | DPI cycle / consumer keys |

Both are discovered automatically via `/dev/input/by-id/` symlinks at startup.

### Keyboard injection (daemon)

```
Rhai script calls press_key / tap_key / type_text
        │
        ▼
  UInputDevice  (/dev/uinput, BUS_USB)
        │
        │  kernel assigns "kbd" handler → /dev/input/eventN
        ▼
  XWayland reads "kbd" handler directly
        │
        ▼
  Focused X11 or Wayland-native window receives key event
```

The `BUS_USB` bus type in the uinput descriptor causes the kernel to assign the
`kbd` handler to the virtual device. XWayland reads `kbd` handler nodes
directly (not through libinput), so key events reach both X11 and Wayland-native
windows.

### Mouse injection (STUBBED)

Mouse injection is not implemented. All Rhai mouse functions are no-ops.
See **MOUSE.md** for what was tried and the correct path forward.

The correct approach is the XDG RemoteDesktop portal (`NotifyPointerButton`,
`ConnectToEIS`) implemented via `ashpd` + `reis`.

### Script execution

```
run_script(path, held: Arc<AtomicBool>, uinput, mouse)
        │
        │  read script source from disk
        │  build Rhai Engine with ScriptContext registered
        │  spawn std::thread
        ▼
  Rhai Engine::eval()
        │
        │  on_progress hook: check held.load() on every op
        │  → returns Some("button released") to abort if held=false
        │
        └─ registered functions call into uinput / mouse via Arc<Mutex<>>
```

Each script runs in its own thread. The `held` `AtomicBool` is shared between
the event loop thread (which sets it `false` on button release) and the script
thread (which checks it via `held()` and via the `on_progress` hook). The hook
fires on every Rhai operation, so loops abort promptly after release.

### Config hot-reload

```
notify::RecommendedWatcher watches ~/.config/macronova/
        │
        │  inotify Modify/Create events on config.toml
        ▼
  Config::load() → replace Arc<Mutex<Config>>
```

The config `Arc<Mutex<Config>>` is re-locked on every button event to look up
bindings, so a newly loaded config takes effect on the next button press.

---

## Threading model

| Thread | Role |
|---|---|
| Main thread | evdev poll loop + config watcher drain |
| `macro:<path>` threads | One per active script; terminate when script ends or `held=false` |

`UInputDevice` and `MouseInjector` are wrapped in `Arc<Mutex<>>` and shared
across macro threads. All Rhai engine state is per-thread (the engine is
constructed fresh for each script invocation).

---

## Config format

`~/.config/macronova/config.toml`

```toml
[device.<name>]
wpid    = "407F"      # Logitech wireless product ID (optional, informational)
usb_pid = "C099"      # USB product ID (optional, informational)

[[device.<name>.bindings]]
button     = "event5/key0x0115"   # <eventN>/key0x<HHHH>
on_press   = "macros/foo.rhai"    # relative to ~/.config/macronova/
on_release = "macros/bar.rhai"    # optional
```

Button names are in the format `<eventN>/key0x<HHHH>`:

- `eventN` is the basename of the `/dev/input/eventN` node (stable via
  `/dev/input/by-id/` symlinks)
- `HHHH` is the four-digit lowercase hex `EV_KEY` code from the evdev event

---

## Key technical findings

### Why uinput virtual mouse doesn't work on Wayland

libinput (used by KWin) hardcodes a check: any device whose sysfs path starts
with `/sys/devices/virtual/input/` is silently ignored. uinput devices always
land under that path regardless of the bus type set in the descriptor.
`BTN_*` codes also don't trigger the `kbd` handler that XWayland reads directly.

### Why XTest doesn't work for native Wayland windows

`XTestFakeButtonEvent` injects into the XWayland event queue. On KDE Plasma 6
virtually all application windows are native Wayland clients. XTest events never
leave XWayland; no Wayland surface receives them.

### Why the virtual keyboard works

XWayland opens and reads `kbd`-handler evdev nodes directly, bypassing libinput.
Setting `BUS_USB` in the uinput descriptor causes the kernel to assign the `kbd`
handler to the virtual device. Key events sent to it are delivered to whichever
window XWayland has focused, including native Wayland windows via XWayland's
Wayland surface.
