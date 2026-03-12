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
  │    └─ xtest.rs      Mouse injector via XDG RemoteDesktop portal + EIS
  └─ engine/
       └─ rhai.rs       Sandboxed Rhai engine, ScriptContext, run_script()

macronova-daemon    (binaries: hidraw-sniffer, evdev-sniffer, mouse-test)
  └─ bin/
       ├─ hidraw_sniffer.rs   Print raw HID++ frames from /dev/hidraw*
       ├─ evdev_sniffer.rs    Print raw evdev events from /dev/input/event*
       └─ mouse_test.rs       Standalone EIS click test / diagnostic tool

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

### Mouse injection (daemon)

```
Rhai script calls click / press_mouse / move_mouse / scroll …
        │
        │  MouseCmd sent via mpsc::SyncSender (non-blocking)
        ▼
  macronova-eis thread  (std::thread, owns EIS socket)
        │
        │  libc::poll on EIS fd — wakes on socket readable OR every 50 ms
        │  drain_eis(): reads EIS events, handles seat/device enumeration,
        │               calls start_emulating on every Virtual device
        │  execute(): sends button/motion/scroll frames to the right device
        ▼
  KDE EIS server → pointer/button event delivered to focused window
```

**Portal session setup** (once at daemon start, blocking):

1. A temporary tokio current-thread runtime runs the async portal flow:
   `RemoteDesktop::new()` → `create_session()` →
   `select_devices(Keyboard | Pointer, persist=Application)` →
   `start()` → `connect_to_eis()` → `ei::Context`
2. `reis::handshake::ei_handshake_blocking()` completes the EIS wire handshake.
3. The worker thread is spawned with the `ei::Context`.
4. `MouseInjector::new()` blocks on a `Condvar` until the worker signals that
   at least one Virtual device has reached `Resumed` state (i.e. is ready to
   accept input). Falls back after a 3-second timeout.

`persist=Application` causes KDE to store the grant; subsequent daemon starts
reuse it without showing the permission dialog again.

**Device selection**: KDE provides three virtual devices — `eis pointer`
(`ei_pointer + ei_button + ei_scroll`), `eis absolute device`
(`ei_pointer_absolute + ei_button + ei_scroll`), and `eis keyboard`
(`ei_keyboard`). Button clicks and relative motion use the `eis pointer`
device; absolute warps use `eis absolute device`. Each command is sent to
exactly one device to avoid duplicate events.

**Graceful degradation**: if the portal is unavailable (no
`DBUS_SESSION_BUS_ADDRESS`, user denied, etc.) `MouseInjector::new()` logs a
warning and returns an injector with `tx = None`; all mouse calls become
silent no-ops so the daemon continues running for keyboard macros.

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
        └─ registered functions call into uinput / mouse via Arc<Mutex<>>
           held() checks the AtomicBool directly
```

Each script runs in its own thread. The `held` `AtomicBool` is shared between
the event loop thread (which sets it `false` on button release) and the script
thread (which checks it via `held()`). Scripts are not interrupted mid-execution
— termination happens only at `while held()` loop boundaries, ensuring
press/release pairs are always completed.

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
| `macronova-eis` | Owns EIS socket; drains EIS events and executes mouse commands |
| `macro:<path>` threads | One per active script; terminate when script ends or `held()` returns false |

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

### Why enigo 0.6 libei clicks were silently dropped

`enigo 0.6`'s `Enigo::new()` calls `start_emulating` only on devices already
`Resumed` at the moment `new()` returns. KDE's EIS server sends the
pointer/button virtual device asynchronously — it arrives after `new()` has
returned. With no further event loop running, `self.devices` never contained a
device with an `ei_button` interface, so the `if let Some(...)` guard silently
fell through on every click. The keyboard worked by coincidence (its device
arrived in time during the synchronous init). This is why enigo was replaced
with a direct `ashpd` + `reis` implementation.

### Why the virtual keyboard works

XWayland opens and reads `kbd`-handler evdev nodes directly, bypassing libinput.
Setting `BUS_USB` in the uinput descriptor causes the kernel to assign the `kbd`
handler to the virtual device. Key events sent to it are delivered to whichever
window XWayland has focused, including native Wayland windows via XWayland's
Wayland surface.

### Why the EIS worker uses a Condvar at startup

After the portal handshake completes, KDE sends seat and device descriptors
asynchronously. If a macro fires immediately after daemon start (before the
worker has processed the first `Resumed` event), `self.devices` is empty and
clicks are dropped. The `Condvar` in `MouseInjector::try_new()` blocks the
calling thread until the worker signals that at least one device is emulating,
guaranteeing the injector is usable before `new()` returns.
