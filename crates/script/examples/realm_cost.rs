//! Measures JavaScript realm creation time and memory (see ADR-0004).
//! `cargo run --release -p rustlite-script --example realm_cost -- 50`

use std::time::{Duration, Instant};
fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(0);
    let start = Instant::now();
    let hosts: Vec<_> = (0..n)
        .map(|_| rustlite_script::ScriptHost::new("https://a.test/"))
        .collect();
    if n > 0 {
        println!(
            "realm create: {:.3} ms each",
            start.elapsed().as_secs_f64() * 1000.0 / n as f64
        );
    }
    println!("ready");
    std::thread::sleep(Duration::from_secs(3));
    drop(hosts);
}
