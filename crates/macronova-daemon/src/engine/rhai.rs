/// Rhai scripting engine setup for macro execution.
///
/// Exposes a sandboxed Rhai Engine with the MacroNova API:
///   press_key(name)        - hold a keyboard key down
///   release_key(name)      - release a keyboard key
///   tap_key(name)          - press and immediately release a keyboard key
///   type_text(text)        - type a string
///   click(btn)             - click a mouse button ("left","right","middle","side","extra")
///   press_mouse(btn)       - hold a mouse button down
///   release_mouse(btn)     - release a mouse button
///   move_mouse(dx, dy)     - move cursor relative by (dx, dy) pixels
///   warp_mouse(x, y)       - warp cursor to absolute screen position (x, y)
///   scroll(amount)         - vertical scroll (positive = down, enigo convention)
///   hscroll(amount)        - horizontal scroll (positive = right)
///   sleep(ms)              - sleep for N milliseconds
///   held()                 - returns true while the trigger button is still held
///
/// Scripts run in a dedicated thread. The held() function checks a shared
/// AtomicBool that the daemon sets to false on button release.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use anyhow::{Context, Result};
use rhai::Engine;
use tracing::{debug, error, warn};

use crate::input::uinput::{key_by_name, UInputDevice};
use crate::input::xtest::{xbtn_by_name, MouseInjector};

/// Shared state passed into Rhai registered functions.
#[derive(Clone)]
pub struct ScriptContext {
    /// Set to false when the trigger button is released.
    pub held: Arc<AtomicBool>,
    /// Virtual keyboard for injecting key events.
    pub uinput: Arc<Mutex<UInputDevice>>,
    /// Mouse injector stub (see MOUSE.md — not yet implemented).
    pub mouse: Arc<Mutex<MouseInjector>>,
}

/// Build a sandboxed Rhai Engine with the MacroNova API registered.
pub fn build_engine(ctx: ScriptContext) -> Engine {
    let mut engine = Engine::new();

    // Sandbox: restrict operations to prevent runaway scripts.
    engine.set_max_operations(0); // unlimited — termination is via held() in script loops
    engine.set_max_call_levels(64);
    engine.set_max_string_size(4096);
    engine.set_max_array_size(1024);
    engine.set_max_map_size(256);

    // held() → bool: returns true while trigger button is pressed.
    {
        let held = Arc::clone(&ctx.held);
        engine.register_fn("held", move || -> bool { held.load(Ordering::Relaxed) });
    }

    // sleep(ms: int): sleep for N milliseconds.
    engine.register_fn("sleep", |ms: i64| {
        std::thread::sleep(Duration::from_millis(ms.max(0) as u64));
    });

    // press_key(name: &str): hold a key down.
    {
        let uinput = Arc::clone(&ctx.uinput);
        engine.register_fn("press_key", move |name: &str| match key_by_name(name) {
            Some(key) => {
                if let Ok(mut dev) = uinput.lock() {
                    if let Err(e) = dev.key_down(key) {
                        warn!("press_key({}) failed: {}", name, e);
                    }
                }
            }
            None => warn!("press_key: unknown key '{}'", name),
        });
    }

    // release_key(name: &str): release a held key.
    {
        let uinput = Arc::clone(&ctx.uinput);
        engine.register_fn("release_key", move |name: &str| match key_by_name(name) {
            Some(key) => {
                if let Ok(mut dev) = uinput.lock() {
                    if let Err(e) = dev.key_up(key) {
                        warn!("release_key({}) failed: {}", name, e);
                    }
                }
            }
            None => warn!("release_key: unknown key '{}'", name),
        });
    }

    // tap_key(name: &str): press and immediately release.
    {
        let uinput = Arc::clone(&ctx.uinput);
        engine.register_fn("tap_key", move |name: &str| match key_by_name(name) {
            Some(key) => {
                if let Ok(mut dev) = uinput.lock() {
                    if let Err(e) = dev.tap_key(key) {
                        warn!("tap_key({}) failed: {}", name, e);
                    }
                }
            }
            None => warn!("tap_key: unknown key '{}'", name),
        });
    }

    // type_text(text: &str): type a string character by character.
    {
        let uinput = Arc::clone(&ctx.uinput);
        engine.register_fn("type_text", move |text: &str| {
            if let Ok(mut dev) = uinput.lock() {
                if let Err(e) = dev.type_text(text) {
                    warn!("type_text failed: {}", e);
                }
            }
        });
    }

    // click(btn: &str): click a mouse button once.
    {
        let mouse = Arc::clone(&ctx.mouse);
        engine.register_fn("click", move |btn: &str| match xbtn_by_name(btn) {
            Some(b) => mouse.lock().unwrap().click(b),
            None => warn!("click: unknown button '{}'", btn),
        });
    }

    // press_mouse(btn: &str): hold a mouse button down.
    {
        let mouse = Arc::clone(&ctx.mouse);
        engine.register_fn("press_mouse", move |btn: &str| match xbtn_by_name(btn) {
            Some(b) => mouse.lock().unwrap().button_event(b, true),
            None => warn!("press_mouse: unknown button '{}'", btn),
        });
    }

    // release_mouse(btn: &str): release a mouse button.
    {
        let mouse = Arc::clone(&ctx.mouse);
        engine.register_fn("release_mouse", move |btn: &str| match xbtn_by_name(btn) {
            Some(b) => mouse.lock().unwrap().button_event(b, false),
            None => warn!("release_mouse: unknown button '{}'", btn),
        });
    }

    // move_mouse(dx: int, dy: int): move cursor relatively.
    {
        let mouse = Arc::clone(&ctx.mouse);
        engine.register_fn("move_mouse", move |dx: i64, dy: i64| {
            mouse.lock().unwrap().move_rel(dx as i32, dy as i32);
        });
    }

    // warp_mouse(x: int, y: int): warp cursor to absolute position.
    {
        let mouse = Arc::clone(&ctx.mouse);
        engine.register_fn("warp_mouse", move |x: i64, y: i64| {
            mouse.lock().unwrap().move_abs(x as i32, y as i32);
        });
    }

    // scroll(amount: int): vertical scroll. Positive = down (enigo convention).
    {
        let mouse = Arc::clone(&ctx.mouse);
        engine.register_fn("scroll", move |amount: i64| {
            mouse.lock().unwrap().scroll(amount as i32);
        });
    }

    // hscroll(amount: int): horizontal scroll. Positive = right.
    {
        let mouse = Arc::clone(&ctx.mouse);
        engine.register_fn("hscroll", move |amount: i64| {
            mouse.lock().unwrap().hscroll(amount as i32);
        });
    }

    // run_command(cmd: &str): spawn a shell command (non-blocking).
    engine.register_fn("run_command", |cmd: &str| {
        let cmd = cmd.to_string();
        std::thread::spawn(move || {
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .status();
            debug!("run_command('{}') exited: {:?}", cmd, status);
        });
    });

    engine
}

/// Run a Rhai script file in a blocking thread.
/// The `held` AtomicBool controls loop continuation.
/// The script thread is detached; it terminates when `held` → false or the script ends.
pub fn run_script(
    script_path: &str,
    held: Arc<AtomicBool>,
    uinput: Arc<Mutex<UInputDevice>>,
    mouse: Arc<Mutex<MouseInjector>>,
) -> Result<()> {
    let source = std::fs::read_to_string(script_path)
        .with_context(|| format!("Failed to read macro script: {}", script_path))?;

    let ctx = ScriptContext {
        held: Arc::clone(&held),
        uinput,
        mouse,
    };

    let script_path = script_path.to_string();
    std::thread::Builder::new()
        .name(format!("macro:{}", script_path))
        .spawn(move || {
            let engine = build_engine(ctx);
            debug!("Running macro: {}", script_path);
            match engine.eval::<()>(&source) {
                Ok(_) => debug!("Macro '{}' completed", script_path),
                Err(e) => error!("Macro '{}' error: {}", script_path, e),
            }
        })
        .context("Failed to spawn macro thread")?;

    Ok(())
}
