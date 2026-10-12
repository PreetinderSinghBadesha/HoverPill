//! Prints what the media backend sees, for debugging a player that doesn't show up.
//!   cargo run -p hoverpill-media --example now_playing [seconds]

use std::time::Duration;

fn main() {
    let secs = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let _media = hoverpill_media::Media::spawn(|now| match now {
        Some(n) => println!(
            "{} {} — {}  (artwork: {})",
            if n.playing { "▶" } else { "⏸" },
            n.title,
            n.artist,
            n.artwork.as_deref().map_or("none".into(), |a| format!("{}…, {} chars", &a[..a.len().min(30)], a.len()))
        ),
        None => println!("nothing playing"),
    });
    std::thread::sleep(Duration::from_secs(secs));
}
