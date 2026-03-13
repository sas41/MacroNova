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
///   scroll(amount)         - vertical scroll (positive = down)
///   hscroll(amount)        - horizontal scroll (positive = right)
///   sleep(ms)              - sleep for N milliseconds
///   held()                 - returns true while the trigger button is still held
///   get_mouse_pos()        - returns [x, y] array of current cursor position, or [-1, -1] on failure
///   log(msg)               - log a message at INFO level
///
/// All input injection is delegated to the platform `InputInjector` trait.
/// Scripts run in a dedicated thread. The held() function checks a shared
/// AtomicBool that the daemon sets to false on button release.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use anyhow::{Context, Result};
use macronova_core::platform::input::get_cursor_position;
use macronova_core::platform::input::InputInjector;
use rhai::Engine;
use tracing::{debug, error, info, warn};

/// Shared state passed into Rhai registered functions.
#[derive(Clone)]
pub struct ScriptContext {
    /// Set to false when the trigger button is released.
    pub held: Arc<AtomicBool>,
    /// Platform input injector.
    pub injector: Arc<Mutex<dyn InputInjector>>,
}

/// Build a sandboxed Rhai Engine with the MacroNova API registered.
pub fn build_engine(ctx: ScriptContext) -> Engine {
    let mut engine = Engine::new();

    engine.set_max_operations(0);
    engine.set_max_call_levels(64);
    engine.set_max_string_size(4096);
    engine.set_max_array_size(1024);
    engine.set_max_map_size(256);

    // held() → bool
    {
        let held = Arc::clone(&ctx.held);
        engine.register_fn("held", move || -> bool { held.load(Ordering::Relaxed) });
    }

    // sleep(ms: int)
    engine.register_fn("sleep", |ms: i64| {
        std::thread::sleep(Duration::from_millis(ms.max(0) as u64));
    });

    // press_key / release_key / tap_key / type_text
    {
        let inj = Arc::clone(&ctx.injector);
        engine.register_fn("press_key", move |name: &str| {
            if let Err(e) = inj.lock().unwrap().key_down(name) {
                warn!("press_key('{}') failed: {}", name, e);
            }
        });
    }
    {
        let inj = Arc::clone(&ctx.injector);
        engine.register_fn("release_key", move |name: &str| {
            if let Err(e) = inj.lock().unwrap().key_up(name) {
                warn!("release_key('{}') failed: {}", name, e);
            }
        });
    }
    {
        let inj = Arc::clone(&ctx.injector);
        engine.register_fn("tap_key", move |name: &str| {
            if let Err(e) = inj.lock().unwrap().tap_key(name) {
                warn!("tap_key('{}') failed: {}", name, e);
            }
        });
    }
    {
        let inj = Arc::clone(&ctx.injector);
        engine.register_fn("type_text", move |text: &str| {
            if let Err(e) = inj.lock().unwrap().type_text(text) {
                warn!("type_text failed: {}", e);
            }
        });
    }

    // click / press_mouse / release_mouse
    {
        let inj = Arc::clone(&ctx.injector);
        engine.register_fn("click", move |btn: &str| {
            if let Err(e) = inj.lock().unwrap().click(btn) {
                warn!("click('{}') failed: {}", btn, e);
            }
        });
    }
    {
        let inj = Arc::clone(&ctx.injector);
        engine.register_fn("press_mouse", move |btn: &str| {
            if let Err(e) = inj.lock().unwrap().button_down(btn) {
                warn!("press_mouse('{}') failed: {}", btn, e);
            }
        });
    }
    {
        let inj = Arc::clone(&ctx.injector);
        engine.register_fn("release_mouse", move |btn: &str| {
            if let Err(e) = inj.lock().unwrap().button_up(btn) {
                warn!("release_mouse('{}') failed: {}", btn, e);
            }
        });
    }

    // move_mouse / warp_mouse / scroll / hscroll
    {
        let inj = Arc::clone(&ctx.injector);
        engine.register_fn("move_mouse", move |dx: i64, dy: i64| {
            if let Err(e) = inj.lock().unwrap().move_rel(dx as i32, dy as i32) {
                warn!("move_mouse failed: {}", e);
            }
        });
    }
    {
        let inj = Arc::clone(&ctx.injector);
        engine.register_fn("warp_mouse", move |x: i64, y: i64| {
            if let Err(e) = inj.lock().unwrap().warp(x as i32, y as i32) {
                warn!("warp_mouse failed: {}", e);
            }
        });
    }
    {
        let inj = Arc::clone(&ctx.injector);
        engine.register_fn("scroll", move |amount: i64| {
            if let Err(e) = inj.lock().unwrap().scroll(amount as i32) {
                warn!("scroll failed: {}", e);
            }
        });
    }
    {
        let inj = Arc::clone(&ctx.injector);
        engine.register_fn("hscroll", move |amount: i64| {
            if let Err(e) = inj.lock().unwrap().hscroll(amount as i32) {
                warn!("hscroll failed: {}", e);
            }
        });
    }

    // get_mouse_pos() → [x, y]: returns the current cursor position.
    // Returns [-1, -1] if the display server is unavailable.
    engine.register_fn("get_mouse_pos", || -> rhai::Array {
        match get_cursor_position() {
            Some((x, y)) => vec![rhai::Dynamic::from(x as i64), rhai::Dynamic::from(y as i64)],
            None => vec![rhai::Dynamic::from(-1i64), rhai::Dynamic::from(-1i64)],
        }
    });

    // log(msg: &str): emit a message at INFO level via the daemon's logger.
    engine.register_fn("log", |msg: &str| {
        info!("[macro] {}", msg);
    });

    // run_command(cmd: &str): spawn a shell command (non-blocking).
    engine.register_fn("run_command", |cmd: &str| {
        let cmd = cmd.to_string();
        std::thread::spawn(move || {
            #[cfg(not(target_os = "windows"))]
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .status();
            #[cfg(target_os = "windows")]
            let status = std::process::Command::new("cmd")
                .args(["/C", &cmd])
                .status();
            debug!("run_command('{}') exited: {:?}", cmd, status);
        });
    });

    engine
}

/// Run a Rhai script file in a dedicated thread.
pub fn run_script(
    script_path: &str,
    held: Arc<AtomicBool>,
    injector: Arc<Mutex<dyn InputInjector>>,
) -> Result<()> {
    let source = std::fs::read_to_string(script_path)
        .with_context(|| format!("Failed to read macro script: {}", script_path))?;

    let ctx = ScriptContext { held, injector };

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
