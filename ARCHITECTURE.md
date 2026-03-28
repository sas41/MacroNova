# Architecture

## Crate overview

```
macronova-core      (library)
  ├─ config.rs              Config / InputDeviceConfig / ButtonBinding / WarpMode
  ├─ platform/
  │    └─ input.rs          InputInjector trait, cursor + desktop helpers
  └─ device/
       ├─ evdev_input.rs    Raw evdev reader + by-id candidate discovery
       ├─ hidraw_input.rs   HID++ raw input helpers (debug/research)
       ├─ logitech/         Logitech discovery helpers
       └─ hidpp/            HID++ protocol helpers

macronova-daemon    (binary: macronova-daemon)
  ├─ main.rs                Multi-device evdev event loop, config hot-reload
  ├─ engine/rhai.rs         Script runtime
  └─ input/platform/linux/  uinput injector

macronova-gui       (binary: macronova-gui)
  ├─ app.rs                 App shell + preview event thread
  └─ views/
       ├─ bindings.rs       Device picker, per-device bindings, scoped capture
       ├─ devices.rs        HID++ info panel
       ├─ editor.rs         Rhai file editor
       ├─ daemon.rs         Service setup/status
       └─ settings.rs       Global settings
```

---

## Core design

- Config is explicit and device-first: `[[devices]]` entries define the exact input source paths to open.
- GUI stores stable `/dev/input/by-id/...` paths (not canonical `/dev/input/eventN`) so kernel/device renumbering is handled automatically.
- Bindings live under each configured device (`[[devices.bindings]]`).
- Captured button identifiers are fully scoped: `device_id::node/key0xNNNN`.
- Daemon opens only configured evdev devices; there is no implicit hardcoded device fallback.
- Daemon no longer runs a HID++ notification diversion path.

---

## Input flow (Linux)

```
Configured device paths (mouse_path [+ optional kbd_path])
        │
        ▼
EvdevReader::open() for each configured device
        │
        ▼
poll() -> DeviceEvent::Button / DeviceEvent::Passthrough
        │
        ├─ Button:
        │    name = "<device_id>::<node>/key0xNNNN"
        │    lookup binding by exact scoped name
        │    run on_press / on_release scripts
        │
        └─ Passthrough:
             when grabbed, forward via uinput passthrough
```

`EVIOCGRAB` remains global per opened device fd. If Virtual Mode is enabled, non-intercepted events are re-injected via uinput so normal device behavior continues.

---

## GUI flow

Bindings tab:

1. User clicks `+ Add device`.
2. GUI lists by-id candidates from `list_evdev_device_candidates()`.
3. User selects one candidate; config gains one `[[devices]]` entry.
4. User adds bindings under that device.
5. Capture on a binding row accepts only events from that same `device_id`.

This prevents cross-device capture pollution when multiple devices emit similar event codes.

---

## Config format

`~/.config/macronova/config.toml`

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
on_release = "macros/back_release.rhai"
intercept = false
```

This is a breaking change from legacy `[device.<name>]` format.

---

## Threading model

| Thread | Role |
|---|---|
| Daemon main thread | Config watcher + polling each configured evdev reader |
| `macro:<path>` threads | One thread per active script invocation |
| GUI preview thread | Poll configured devices and feed capture/status |

`InputInjector` is shared with `Arc<Mutex<dyn InputInjector>>` in the daemon runtime.
