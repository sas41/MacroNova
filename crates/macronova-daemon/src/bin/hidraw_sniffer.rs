/// hidraw-sniffer: reads /dev/hidraw16, hidraw17, and hidraw18 simultaneously
/// and prints notable packets with a timestamp.
///
/// For hidraw16: shows any packet where bytes [0..2] (button bitmask) change from previous value,
///              OR where the button bitmask is non-zero.
/// For hidraw17 and hidraw18: shows ALL non-zero-data packets.
///
/// Usage:
///   cargo run --bin hidraw-sniffer
///
/// Press Back, Forward, Sniper, and DPI buttons to identify the button byte layout.
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::thread;
use std::time::Instant;

fn sniff_hidraw16(start: Instant) {
    let path = "/dev/hidraw16";
    let mut f = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[{path}] open error: {e}");
            return;
        }
    };
    println!("[{path}] opened OK (showing button bitmask changes at bytes [0..2])");
    println!("[{path}] Bit legend: b0=LEFT b1=RIGHT b2=MIDDLE b3=? b4=? b5=? ...");

    let mut buf = [0u8; 64];
    let mut last_buttons: u16 = 0;
    let deadline = std::time::Duration::from_secs(90);

    loop {
        if start.elapsed() > deadline {
            println!("[{path}] done");
            break;
        }
        match f.read(&mut buf) {
            Ok(0) | Err(_) => {
                thread::sleep(std::time::Duration::from_millis(1));
            }
            Ok(n) => {
                let data = &buf[..n];
                if data.iter().all(|&b| b == 0) {
                    continue;
                }
                if n < 2 {
                    continue;
                }
                let buttons = u16::from_le_bytes([data[0], data[1]]);
                if buttons != last_buttons {
                    let bits: String = (0u8..16)
                        .map(|i| if buttons & (1 << i) != 0 { '1' } else { '0' })
                        .collect();
                    // Identify which new bit(s) appeared vs disappeared
                    let pressed: Vec<u8> = (0..16u8)
                        .filter(|&i| buttons & (1 << i) != 0 && last_buttons & (1 << i) == 0)
                        .collect();
                    let released: Vec<u8> = (0..16u8)
                        .filter(|&i| last_buttons & (1 << i) != 0 && buttons & (1 << i) == 0)
                        .collect();
                    let event = if !pressed.is_empty() {
                        format!("DOWN bits {:?}", pressed)
                    } else if !released.is_empty() {
                        format!("UP   bits {:?}", released)
                    } else {
                        format!("CHANGE")
                    };
                    println!(
                        "[hidraw16] +{:.3}s  btns={:#06x} bits=[{}]  {}",
                        start.elapsed().as_secs_f32(),
                        buttons,
                        bits,
                        event,
                    );
                    last_buttons = buttons;
                }
            }
        }
    }
}

fn sniff_generic(path: &str, start: Instant) {
    let path = path.to_string();
    let mut f = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[{path}] open error: {e}");
            return;
        }
    };
    println!("[{path}] opened OK (showing all non-zero packets)");

    let mut buf = [0u8; 64];
    let deadline = std::time::Duration::from_secs(90);

    loop {
        if start.elapsed() > deadline {
            println!("[{path}] done");
            break;
        }
        match f.read(&mut buf) {
            Ok(0) | Err(_) => {
                thread::sleep(std::time::Duration::from_millis(1));
            }
            Ok(n) => {
                let data = &buf[..n];
                if data.iter().all(|&b| b == 0) {
                    continue;
                }
                let hex: Vec<String> = data.iter().map(|b| format!("{b:02x}")).collect();
                println!(
                    "[{path}] +{:.3}s  len={n}  {}",
                    start.elapsed().as_secs_f32(),
                    hex.join(" "),
                );
            }
        }
    }
}

fn main() {
    let start = Instant::now();

    let h16 = {
        let s = start;
        thread::spawn(move || sniff_hidraw16(s))
    };
    let h17 = {
        let s = start;
        thread::spawn(move || sniff_generic("/dev/hidraw17", s))
    };
    let h18 = {
        let s = start;
        thread::spawn(move || sniff_generic("/dev/hidraw18", s))
    };

    println!("=== hidraw sniffer — 60 seconds ===");
    println!("Press ONE button at a time, hold 1 second, release:");
    println!("  1. Left click");
    println!("  2. Right click");
    println!("  3. Middle/wheel click");
    println!("  4. Back button (side, thumb area)");
    println!("  5. Forward button (side, thumb area)");
    println!("  6. Sniper button (left side, under thumb)");
    println!("  7. DPI+ button (top of mouse)");
    println!("  8. DPI- button (top of mouse)");

    let _ = h16.join();
    let _ = h17.join();
    let _ = h18.join();
    println!("Done.");
}
