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
///   warp_mouse(x, y)       - no-op (absolute positioning not supported via uinput)
///   scroll(amount)         - vertical scroll (positive = down)
///   hscroll(amount)        - horizontal scroll (positive = right)
///   sleep(ms)              - sleep for N milliseconds
///   held()                 - returns true while the trigger button is still held
///
/// All input injection goes through the uinput virtual device.
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

use crate::input::uinput::{btn_by_name, key_by_name, UInputDevice};

/// Shared state passed into Rhai registered functions.
#[derive(Clone)]
pub struct ScriptContext {
    /// Set to false when the trigger button is released.
    pub held: Arc<AtomicBool>,
    /// Virtual input device for injecting key and mouse events.
    pub uinput: Arc<Mutex<UInputDevice>>,
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
                if let Err(e) = uinput.lock().unwrap().key_down(key) {
                    warn!("press_key({}) failed: {}", name, e);
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
                if let Err(e) = uinput.lock().unwrap().key_up(key) {
                    warn!("release_key({}) failed: {}", name, e);
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
                if let Err(e) = uinput.lock().unwrap().tap_key(key) {
                    warn!("tap_key({}) failed: {}", name, e);
                }
            }
            None => warn!("tap_key: unknown key '{}'", name),
        });
    }

    // type_text(text: &str): type a string character by character.
    {
        let uinput = Arc::clone(&ctx.uinput);
        engine.register_fn("type_text", move |text: &str| {
            if let Err(e) = uinput.lock().unwrap().type_text(text) {
                warn!("type_text failed: {}", e);
            }
        });
    }

    // click(btn: &str): click a mouse button once.
    {
        let uinput = Arc::clone(&ctx.uinput);
        engine.register_fn("click", move |btn: &str| match btn_by_name(btn) {
            Some(code) => {
                if let Err(e) = uinput.lock().unwrap().button_click(code) {
                    warn!("click({}) failed: {}", btn, e);
                }
            }
            None => warn!("click: unknown button '{}'", btn),
        });
    }

    // press_mouse(btn: &str): hold a mouse button down.
    {
        let uinput = Arc::clone(&ctx.uinput);
        engine.register_fn("press_mouse", move |btn: &str| match btn_by_name(btn) {
            Some(code) => {
                if let Err(e) = uinput.lock().unwrap().button_down(code) {
                    warn!("press_mouse({}) failed: {}", btn, e);
                }
            }
            None => warn!("press_mouse: unknown button '{}'", btn),
        });
    }

    // release_mouse(btn: &str): release a mouse button.
    {
        let uinput = Arc::clone(&ctx.uinput);
        engine.register_fn("release_mouse", move |btn: &str| match btn_by_name(btn) {
            Some(code) => {
                if let Err(e) = uinput.lock().unwrap().button_up(code) {
                    warn!("release_mouse({}) failed: {}", btn, e);
                }
            }
            None => warn!("release_mouse: unknown button '{}'", btn),
        });
    }

    // move_mouse(dx: int, dy: int): move cursor relatively.
    {
        let uinput = Arc::clone(&ctx.uinput);
        engine.register_fn("move_mouse", move |dx: i64, dy: i64| {
            if let Err(e) = uinput.lock().unwrap().move_rel(dx as i32, dy as i32) {
                warn!("move_mouse failed: {}", e);
            }
        });
    }

    // warp_mouse(x: int, y: int): absolute positioning — not supported via uinput.
    engine.register_fn("warp_mouse", |_x: i64, _y: i64| {
        warn!("warp_mouse is not supported (requires display server integration)");
    });

    // scroll(amount: int): vertical scroll. Positive = down.
    {
        let uinput = Arc::clone(&ctx.uinput);
        engine.register_fn("scroll", move |amount: i64| {
            if let Err(e) = uinput.lock().unwrap().scroll(amount as i32) {
                warn!("scroll failed: {}", e);
            }
        });
    }

    // hscroll(amount: int): horizontal scroll. Positive = right.
    {
        let uinput = Arc::clone(&ctx.uinput);
        engine.register_fn("hscroll", move |amount: i64| {
            if let Err(e) = uinput.lock().unwrap().hscroll(amount as i32) {
                warn!("hscroll failed: {}", e);
            }
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

/// Run a Rhai script file in a dedicated thread.
/// The `held` AtomicBool controls loop continuation.
/// The script thread is detached; it terminates when `held` → false or the script ends.
pub fn run_script(
    script_path: &str,
    held: Arc<AtomicBool>,
    uinput: Arc<Mutex<UInputDevice>>,
) -> Result<()> {
    let source = std::fs::read_to_string(script_path)
        .with_context(|| format!("Failed to read macro script: {}", script_path))?;

    let ctx = ScriptContext { held, uinput };

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
