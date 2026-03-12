/// evdev-sniffer: reads /dev/input/event12 (Logitech USB Receiver mouse)
/// and /dev/input/event13 (Logitech USB Receiver keyboard/consumer) simultaneously,
/// printing all events for 30 seconds.
///
/// Usage:
///   cargo run --bin evdev-sniffer
///
/// Press Back, Forward, Sniper, and DPI buttons on the mouse to identify
/// which event device and which key code carries each side button.
use evdev::{Device, EventType};
use std::thread;
use std::time::Instant;

fn sniff(path: &str) {
    let path = path.to_string();
    let start = Instant::now();
    let deadline = std::time::Duration::from_secs(30);

    let mut dev = match Device::open(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[{path}] open error: {e}");
            return;
        }
    };

    println!("[{path}] opened: {}", dev.name().unwrap_or("(unknown)"));

    // Set non-blocking so we can check the timeout.
    if let Err(e) = dev.set_nonblocking(true) {
        eprintln!("[{path}] set_nonblocking error: {e}");
        return;
    }

    loop {
        if start.elapsed() > deadline {
            println!("[{path}] done (30 s timeout)");
            break;
        }

        match dev.fetch_events() {
            Ok(events) => {
                for ev in events {
                    // Skip EV_SYN and EV_MSC (noise)
                    if ev.event_type() == EventType::SYNCHRONIZATION
                        || ev.event_type() == EventType::MISC
                    {
                        continue;
                    }
                    // Skip relative axis == 0 (no movement)
                    if ev.event_type() == EventType::RELATIVE && ev.value() == 0 {
                        continue;
                    }
                    println!(
                        "[{path}] +{:.3}s  type={:?}({})  code={}  value={}",
                        start.elapsed().as_secs_f32(),
                        ev.event_type(),
                        ev.event_type().0,
                        ev.code(),
                        ev.value(),
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(e) => {
                eprintln!("[{path}] read error: {e}");
                break;
            }
        }
    }
}

fn main() {
    let paths = ["/dev/input/event12", "/dev/input/event13"];

    let handles: Vec<_> = paths
        .iter()
        .map(|p| {
            let p = p.to_string();
            thread::spawn(move || sniff(&p))
        })
        .collect();

    println!("Sniffer running for 30 seconds. Press ALL side buttons on the mouse now...");
    println!("(Back, Forward, Sniper, DPI cycle, middle click, left, right — identify each)");
    for h in handles {
        let _ = h.join();
    }
    println!("Done.");
}
