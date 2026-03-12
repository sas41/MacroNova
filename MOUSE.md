# Mouse Injection — Status and Research Notes

Mouse click/button injection is currently **stubbed out** (no-op). This document explains what was tried, why it failed, and what the correct path forward is.

---

## What was tried

### 1. uinput virtual mouse device

A `UInputDevice` with `BTN_*` codes was created. This failed for two reasons:

- `BTN_*` codes (0x110+) do not trigger the kernel's `kbd` handler, only `KEY_*` codes 1–255 do. The `kbd` handler is what XWayland reads directly.
- libinput (used by KWin/Wayland) hardcodes a check: any device whose sysfs path starts with `/sys/devices/virtual/input/` is **silently ignored**. This is confirmed by the literal string in `/usr/lib/libinput.so`. Setting `BUS_USB` in the uinput descriptor does **not** change the sysfs path — the kernel still places the device under `/devices/virtual/input/`. The bus type only affects handler assignment, not the sysfs path.

The virtual keyboard (`macronova-kbd`) works because XWayland reads from the `kbd` handler directly, bypassing libinput entirely. Mouse buttons have no equivalent bypass.

### 2. XTest extension via XWayland (`DISPLAY=:0`)

`XTestFakeButtonEvent` via `libXtst` was implemented (`src/input/xtest.rs`) using the `x11-dl` crate. The `XTestInjector` connects to `DISPLAY=:0`, verifies the XTEST extension is present, and calls `XTestFakeButtonEvent` + `XFlush`.

This also fails: **XTest events only reach X11 client windows**. On KDE Plasma Wayland, virtually all application windows are native Wayland clients — not X11 windows. `xlsclients` returns nothing. The active window ID from `_NET_ACTIVE_WINDOW` has no `WM_NAME`/`WM_CLASS` properties because it is a Wayland surface, not an X11 window. XTest events injected into XWayland go into a queue that no Wayland surface is listening to.

Tested with a standalone C program (`xte`-style): the program runs without error, `XFlush` succeeds, but no click is delivered to any focused Wayland window.

---

## The correct solution: XDG RemoteDesktop Portal

The standard Wayland mechanism for input injection is the **XDG RemoteDesktop portal** (`org.freedesktop.portal.RemoteDesktop`), which on KDE Plasma is backed by `xdg-desktop-portal-kde`.

The portal exposes:
- `NotifyPointerButton(session, options, button, state)` — inject mouse button press/release
- `NotifyPointerMotion(session, options, dx, dy)` — relative motion
- `NotifyPointerMotionAbsolute(session, options, stream, x, y)` — absolute warp
- `NotifyPointerAxis(session, options, dx, dy)` — scroll
- `ConnectToEIS(session, options)` → fd — direct libei socket (more efficient for high-frequency events)

The flow requires:
1. `CreateSession` → get a session object path
2. `SelectDevices(session, {types: pointer|keyboard})` → request device types
3. `Start(session, ...)` → **triggers a one-time user permission dialog**
4. After approval, use `NotifyPointer*` calls freely for the lifetime of the session
5. With `persist_mode = persistent`, a restore token is returned so the session can be resumed on next daemon start without a new dialog

### Relevant crates

- **`ashpd`** (`0.13.x`) — high-level async Rust wrapper for XDG portals, including `RemoteDesktop`. Uses `zbus` for D-Bus. Requires `tokio` or `async-std`.
- **`reis`** (`0.6.x`) — pure Rust implementation of the libei/libeis wire protocol. Used with `ConnectToEIS` for direct EI communication. More efficient for rapid repeated events (e.g. click-repeat macros).

The `reis` example (`examples/type-text.rs`) demonstrates the full portal + EI flow. KDE Plasma 6 supports `AvailableDeviceTypes = 7` (keyboard + pointer + touchscreen).

### KDE-specific note

KDE also supports `ConnectToEIS` on the `org.freedesktop.portal.InputCapture` interface (for capture), but for *injection* the `RemoteDesktop` portal is the right interface.

---

## Current state of the code

- `src/input/xtest.rs` — XTest implementation is **kept but stubbed**: all methods are no-ops that log a warning. The `x11-dl` dependency has been removed.
- `src/engine/rhai.rs` — Mouse Rhai functions (`click`, `press_mouse`, `release_mouse`, `move_mouse`, `scroll`, `hscroll`, `warp_mouse`) are **registered but do nothing** — they log a `warn!` when called.
- `main.rs` — `XTestInjector` is replaced with a `MouseInjector` stub that is `Arc<Mutex<…>>`-compatible.

---

## What needs to be implemented

1. Add `ashpd` (with `tokio` feature) and/or `reis` to `macronova-daemon/Cargo.toml`.
2. At daemon startup, establish a `RemoteDesktop` portal session (async, with `tokio::spawn`).
3. On first run, handle the permission dialog (the user must approve once). Store the restore token in `~/.config/macronova/portal_token` to skip the dialog on future runs.
4. Replace the `MouseInjector` stub with a real implementation that calls `NotifyPointerButton` / `ConnectToEIS`.
5. For high-frequency use cases (click-repeat), prefer the EI socket path via `reis` to avoid per-event D-Bus round trips.
