//! Prints two read-only system/network samples, without writing configuration.

use std::io;
use std::time::Duration;
use traffic_monitor_windows::{network, system};

fn main() -> io::Result<()> {
    let previous = system::cpu_times()?;
    println!("CPU baseline: {previous:?}");
    println!("Memory: {:?}", system::memory_status()?);
    for adapter in network::snapshot()? {
        println!("Network: {adapter:?}");
    }
    std::thread::sleep(Duration::from_millis(250));
    let current = system::cpu_times()?;
    println!("CPU current: {current:?}");
    println!("CPU usage: {:?}%", system::cpu_usage(previous, current));
    println!("Memory: {:?}", system::memory_status()?);
    for adapter in network::snapshot()? {
        println!("Network: {adapter:?}");
    }
    Ok(())
}
