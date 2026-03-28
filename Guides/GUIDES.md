# MacroNova Guides

## Current Workflow

MacroNova now uses a device-first workflow:

1. Add a device from the GUI list (`Bindings` -> `+ Add device`).
2. Add bindings under that device.
3. Capture events per binding; capture only accepts events from that selected device.

---

## Device Guides

| Device | Guide |
|---|---|
| Logitech G502 X Lightspeed | [Logitech_G502X_Lightspeed.md](Logitech_G502X_Lightspeed.md) |

---

## Troubleshooting Index

- Buttons work in `evtest` but not in MacroNova: verify the device was added in GUI and bindings were captured under that same device.
- Wrong button captured: remove and recapture from the target device binding row.
- Device not listed in `+ Add device`: check `/dev/input/by-id` entries and input permissions.
