# MacroNova Scripting Reference

Scripts are [Rhai](https://rhai.rs) files (`.rhai`) placed in
`~/.config/macronova/macros/`. Paths in `config.toml` are relative to
`~/.config/macronova/`.

---

## Script API

### Keyboard

| Function | Description |
|---|---|
| `press_key(name)` | Hold a key down |
| `release_key(name)` | Release a held key |
| `tap_key(name)` | Press and immediately release a key |
| `type_text(text)` | Type a string (basic ASCII only) |

### Mouse

| Function | Description |
|---|---|
| `click(btn)` | Press and release a mouse button |
| `press_mouse(btn)` | Hold a mouse button down |
| `release_mouse(btn)` | Release a held mouse button |
| `move_mouse(dx, dy)` | Move cursor relative by (dx, dy) pixels |
| `warp_mouse(x, y)` | Warp cursor to absolute screen position |
| `scroll(amount)` | Vertical scroll — positive = down |
| `hscroll(amount)` | Horizontal scroll — positive = right |

Mouse button names: `"left"`, `"right"`, `"middle"`, `"side"` (back), `"extra"` (forward).

### Control

| Function | Description |
|---|---|
| `sleep(ms)` | Sleep for N milliseconds |
| `held()` | Returns `true` while the trigger button is still held |
| `run_command(cmd)` | Spawn a shell command (non-blocking) |

---

## held() and loop termination

`held()` reflects the physical state of the button that triggered the script.
It becomes `false` the moment the button is released.

The idiomatic pattern for a repeating action:

```rhai
while held() {
    press_mouse("left");
    sleep(15);
    release_mouse("left");
    sleep(15);
}
```

**Important:** MacroNova does not interrupt a script mid-cycle. The loop
condition is only checked at the top of each iteration, so a
`press_mouse` is always paired with its `release_mouse` before the
loop exits. Never split a press/release pair across a `held()` check.

For a one-shot script (no loop) `held()` is not needed:

```rhai
press_key("ctrl");
tap_key("z");
release_key("ctrl");
```

---

## Key names

`press_key`, `release_key`, and `tap_key` accept key names in three forms:

1. **Short alias** — `"ctrl"`, `"enter"`, `"up"` (see alias table below)
2. **Plain name** — `"a"`, `"f5"`, `"volumeup"` — automatically prefixed with `KEY_`
3. **Full evdev name** — `"KEY_LEFTCTRL"`, `"KEY_PLAYPAUSE"` — used verbatim

Names are case-insensitive.

### Aliases

| Alias | Resolves to |
|---|---|
| `ctrl`, `control` | `KEY_LEFTCTRL` |
| `alt` | `KEY_LEFTALT` |
| `shift` | `KEY_LEFTSHIFT` |
| `super`, `win`, `meta` | `KEY_LEFTMETA` |
| `enter`, `return` | `KEY_ENTER` |
| `esc`, `escape` | `KEY_ESC` |
| `space` | `KEY_SPACE` |
| `tab` | `KEY_TAB` |
| `backspace` | `KEY_BACKSPACE` |
| `delete`, `del` | `KEY_DELETE` |
| `up` | `KEY_UP` |
| `down` | `KEY_DOWN` |
| `left` | `KEY_LEFT` |
| `right` | `KEY_RIGHT` |
| `home` | `KEY_HOME` |
| `end` | `KEY_END` |
| `pgup`, `page_up`, `pageup` | `KEY_PAGEUP` |
| `pgdn`, `page_down`, `pagedown` | `KEY_PAGEDOWN` |

---

## Full key reference

All keys usable with `press_key` / `release_key` / `tap_key`. Use either the
short form (strip `KEY_`, lower-case) or the full evdev name.

### Standard keyboard

| Short form | Full name | Notes |
|---|---|---|
| `esc` | `KEY_ESC` | |
| `1`–`0` | `KEY_1`–`KEY_0` | Top-row number keys |
| `minus` | `KEY_MINUS` | `-` |
| `equal` | `KEY_EQUAL` | `=` |
| `backspace` | `KEY_BACKSPACE` | |
| `tab` | `KEY_TAB` | |
| `q`–`p` | `KEY_Q`–`KEY_P` | |
| `leftbrace` | `KEY_LEFTBRACE` | `[` |
| `rightbrace` | `KEY_RIGHTBRACE` | `]` |
| `enter` | `KEY_ENTER` | |
| `leftctrl` | `KEY_LEFTCTRL` | |
| `a`–`l` | `KEY_A`–`KEY_L` | |
| `semicolon` | `KEY_SEMICOLON` | `;` |
| `apostrophe` | `KEY_APOSTROPHE` | `'` |
| `grave` | `KEY_GRAVE` | `` ` `` |
| `leftshift` | `KEY_LEFTSHIFT` | |
| `backslash` | `KEY_BACKSLASH` | `\` |
| `z`–`m` | `KEY_Z`–`KEY_M` | |
| `comma` | `KEY_COMMA` | `,` |
| `dot` | `KEY_DOT` | `.` |
| `slash` | `KEY_SLASH` | `/` |
| `rightshift` | `KEY_RIGHTSHIFT` | |
| `leftalt` | `KEY_LEFTALT` | |
| `space` | `KEY_SPACE` | |
| `capslock` | `KEY_CAPSLOCK` | |
| `f1`–`f10` | `KEY_F1`–`KEY_F10` | |
| `numlock` | `KEY_NUMLOCK` | |
| `scrolllock` | `KEY_SCROLLLOCK` | |
| `rightctrl` | `KEY_RIGHTCTRL` | |
| `rightalt` | `KEY_RIGHTALT` | |
| `home` | `KEY_HOME` | |
| `up` | `KEY_UP` | |
| `pageup` | `KEY_PAGEUP` | |
| `left` | `KEY_LEFT` | |
| `right` | `KEY_RIGHT` | |
| `end` | `KEY_END` | |
| `down` | `KEY_DOWN` | |
| `pagedown` | `KEY_PAGEDOWN` | |
| `insert` | `KEY_INSERT` | |
| `delete` | `KEY_DELETE` | |
| `leftmeta` | `KEY_LEFTMETA` | Left Super / Win |
| `rightmeta` | `KEY_RIGHTMETA` | Right Super / Win |
| `compose` | `KEY_COMPOSE` | Menu / Compose |
| `sysrq` | `KEY_SYSRQ` | Print Screen / SysRq |
| `pause` | `KEY_PAUSE` | |

### Function keys F11–F24

| Short form | Full name |
|---|---|
| `f11` | `KEY_F11` |
| `f12` | `KEY_F12` |
| `f13` | `KEY_F13` |
| `f14` | `KEY_F14` |
| `f15` | `KEY_F15` |
| `f16` | `KEY_F16` |
| `f17` | `KEY_F17` |
| `f18` | `KEY_F18` |
| `f19` | `KEY_F19` |
| `f20` | `KEY_F20` |
| `f21` | `KEY_F21` |
| `f22` | `KEY_F22` |
| `f23` | `KEY_F23` |
| `f24` | `KEY_F24` |

### Numpad

| Short form | Full name | Notes |
|---|---|---|
| `kp0`–`kp9` | `KEY_KP0`–`KEY_KP9` | Numpad digits |
| `kpdot` | `KEY_KPDOT` | Numpad `.` |
| `kpenter` | `KEY_KPENTER` | Numpad Enter |
| `kpplus` | `KEY_KPPLUS` | Numpad `+` |
| `kpminus` | `KEY_KPMINUS` | Numpad `-` |
| `kpasterisk` | `KEY_KPASTERISK` | Numpad `*` |
| `kpslash` | `KEY_KPSLASH` | Numpad `/` |
| `kpequal` | `KEY_KPEQUAL` | Numpad `=` |
| `kpplusminus` | `KEY_KPPLUSMINUS` | Numpad `±` |
| `kpcomma` | `KEY_KPCOMMA` | Numpad `,` |
| `kpleftparen` | `KEY_KPLEFTPAREN` | Numpad `(` |
| `kprightparen` | `KEY_KPRIGHTPAREN` | Numpad `)` |

### Media and volume

| Short form | Full name | Action |
|---|---|---|
| `mute` | `KEY_MUTE` | Toggle mute |
| `volumedown` | `KEY_VOLUMEDOWN` | Volume down |
| `volumeup` | `KEY_VOLUMEUP` | Volume up |
| `playpause` | `KEY_PLAYPAUSE` | Play / pause |
| `nextsong` | `KEY_NEXTSONG` | Next track |
| `previoussong` | `KEY_PREVIOUSSONG` | Previous track |
| `stopcd` | `KEY_STOPCD` | Stop playback |
| `play` | `KEY_PLAY` | Play |
| `pausecd` | `KEY_PAUSECD` | Pause |
| `playcd` | `KEY_PLAYCD` | Play CD |
| `record` | `KEY_RECORD` | Record |
| `rewind` | `KEY_REWIND` | Rewind |
| `fastforward` | `KEY_FASTFORWARD` | Fast-forward |
| `ejectcd` | `KEY_EJECTCD` | Eject |
| `ejectclosecd` | `KEY_EJECTCLOSECD` | Eject / close |
| `closecd` | `KEY_CLOSECD` | Close tray |
| `micmute` | `KEY_MICMUTE` | Mute microphone |
| `media` | `KEY_MEDIA` | Media key |
| `bassboost` | `KEY_BASSBOOST` | Bass boost |

### Brightness and display

| Short form | Full name | Action |
|---|---|---|
| `brightnessdown` | `KEY_BRIGHTNESSDOWN` | Brightness down |
| `brightnessup` | `KEY_BRIGHTNESSUP` | Brightness up |
| `brightness_auto` | `KEY_BRIGHTNESS_AUTO` | Auto brightness |
| `brightness_cycle` | `KEY_BRIGHTNESS_CYCLE` | Cycle brightness |
| `display_off` | `KEY_DISPLAY_OFF` | Turn off display |
| `switchvideomode` | `KEY_SWITCHVIDEOMODE` | Switch video output |
| `kbdillumtoggle` | `KEY_KBDILLUMTOGGLE` | Keyboard backlight toggle |
| `kbdillumdown` | `KEY_KBDILLUMDOWN` | Keyboard backlight down |
| `kbdillumup` | `KEY_KBDILLUMUP` | Keyboard backlight up |

### System

| Short form | Full name | Action |
|---|---|---|
| `power` | `KEY_POWER` | Power button |
| `sleep` | `KEY_SLEEP` | Sleep |
| `wakeup` | `KEY_WAKEUP` | Wake |
| `suspend` | `KEY_SUSPEND` | Suspend |
| `print` | `KEY_PRINT` | Print screen |
| `stop` | `KEY_STOP` | Stop |
| `again` | `KEY_AGAIN` | Redo / Again |
| `undo` | `KEY_UNDO` | Undo |
| `redo` | `KEY_REDO` | Redo |
| `copy` | `KEY_COPY` | Copy |
| `paste` | `KEY_PASTE` | Paste |
| `cut` | `KEY_CUT` | Cut |
| `find` | `KEY_FIND` | Find |
| `open` | `KEY_OPEN` | Open |
| `props` | `KEY_PROPS` | Properties |
| `front` | `KEY_FRONT` | Front |
| `help` | `KEY_HELP` | Help |
| `menu` | `KEY_MENU` | Menu |
| `calc` | `KEY_CALC` | Calculator |
| `mail` | `KEY_MAIL` | Mail |
| `bookmarks` | `KEY_BOOKMARKS` | Bookmarks |
| `computer` | `KEY_COMPUTER` | My Computer |
| `back` | `KEY_BACK` | Browser Back |
| `forward` | `KEY_FORWARD` | Browser Forward |
| `refresh` | `KEY_REFRESH` | Browser Refresh |
| `homepage` | `KEY_HOMEPAGE` | Browser Home |
| `www` | `KEY_WWW` | Browser |
| `search` | `KEY_SEARCH` | Search |
| `scrollup` | `KEY_SCROLLUP` | Scroll up |
| `scrolldown` | `KEY_SCROLLDOWN` | Scroll down |
| `new` | `KEY_NEW` | New |
| `close` | `KEY_CLOSE` | Close |
| `save` | `KEY_SAVE` | Save |
| `send` | `KEY_SEND` | Send |
| `reply` | `KEY_REPLY` | Reply |
| `forwardmail` | `KEY_FORWARDMAIL` | Forward mail |
| `cancel` | `KEY_CANCEL` | Cancel |
| `exit` | `KEY_EXIT` | Exit |
| `file` | `KEY_FILE` | File manager |
| `chat` | `KEY_CHAT` | Chat |
| `finance` | `KEY_FINANCE` | Finance |
| `phone` | `KEY_PHONE` | Phone |
| `rfkill` | `KEY_RFKILL` | RF kill switch |
| `wlan` | `KEY_WLAN` | WLAN toggle |
| `bluetooth` | `KEY_BLUETOOTH` | Bluetooth toggle |

---

## Example macros

### Undo (Ctrl+Z)

```rhai
press_key("ctrl");
tap_key("z");
release_key("ctrl");
```

### Redo (Ctrl+Shift+Z)

```rhai
press_key("ctrl");
press_key("shift");
tap_key("z");
release_key("shift");
release_key("ctrl");
```

### Auto-click while held

```rhai
while held() {
    press_mouse("left");
    sleep(15);
    release_mouse("left");
    sleep(15);
}
```

### Volume up five steps

```rhai
tap_key("volumeup");
tap_key("volumeup");
tap_key("volumeup");
tap_key("volumeup");
tap_key("volumeup");
```

### Next track

```rhai
tap_key("nextsong");
```

### Play/pause toggle

```rhai
tap_key("playpause");
```

### Sniper button — slow DPI while held (keyboard shortcut example)

```rhai
// Tell the application to enter precision mode
press_key("shift");
while held() {
    sleep(50);
}
release_key("shift");
```

### Type a fixed string

```rhai
type_text("Hello, world!");
```

### Open a terminal via shell command

```rhai
run_command("konsole");
```

### Scroll down rapidly while held

```rhai
while held() {
    scroll(3);
    sleep(50);
}
```

### Move mouse in a small circle (demo)

```rhai
let steps = 36;
let i = 0;
while i < steps {
    let angle = i * 10;
    // Rhai doesn't have sin/cos; use move_mouse in a square spiral instead
    move_mouse(5, 0);
    sleep(20);
    i += 1;
}
```

---

## config.toml binding

```toml
[[device.G502X.bindings]]
button     = "event5/key0x0115"   # top side button 1
on_press   = "macros/click_repeat.rhai"
on_release = "macros/notify.rhai"   # optional
```

- `on_press` — script runs when the button is pressed; receives a live `held()` flag
- `on_release` — script runs when the button is released; `held()` is always `false` here

Use the GUI **Capture** button to find the `event5/key0x...` name for any physical button.

---

## Notes

- `type_text` only supports basic ASCII. Non-ASCII characters are silently skipped.
- Key codes above 255 (e.g. `KEY_BUTTONCONFIG`, TV remote keys) are outside the
  range registered on the virtual keyboard device and will not be sent.
- `run_command` is non-blocking — the script continues immediately.
- Scripts have no access to the filesystem, network, or system state beyond the
  functions listed above. Rhai's standard library is available (math, strings, arrays, maps).
- A script that never calls `held()` will run to completion regardless of button
  release. Infinite loops without `held()` will run forever.
