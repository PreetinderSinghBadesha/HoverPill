//! Prints notifications as the backend sees them.
//!   cargo run -p hoverpill-notify --example listen [seconds]

use std::time::Duration;

use hoverpill_notify::Event;

fn main() {
    let secs = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(30);
    let start = std::time::Instant::now();
    hoverpill_notify::spawn(move |event| match event {
        Event::Notification(n) => println!(
            "{:>6.2}s [{}] {} — {}  (icon: {})",
            start.elapsed().as_secs_f32(),
            n.app,
            n.title,
            n.body,
            n.icon.as_ref().map_or("none".into(), |i| format!("{} chars", i.len()))
        ),
        Event::Unavailable(reason) => println!("unavailable: {reason}"),
    });
    println!("listening for {secs}s...");
    std::thread::sleep(Duration::from_secs(secs));
}
