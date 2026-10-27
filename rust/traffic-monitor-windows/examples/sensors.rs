//! Explicit read-only probe; never starts an external monitor or installs a driver.
use std::time::{Duration, Instant};
use traffic_monitor_windows::sensors::SensorSampler;
fn main() {
    let mut sampler = SensorSampler::new();
    let started = Instant::now();
    loop {
        let call = Instant::now();
        let sample = sampler.sample("", "");
        let elapsed = call.elapsed();
        if !sample
            .diagnostics
            .iter()
            .any(|d| d.contains("discovery is running"))
            || started.elapsed() > Duration::from_secs(15)
        {
            println!(
                "sample_call_us={} elapsed_ms={}",
                elapsed.as_micros(),
                started.elapsed().as_millis()
            );
            println!(
                "cpu={:?} gpu={:?} disk={:?} board={:?} cpu_sensors={} disks={}",
                sample.cpu_temperature,
                sample.gpu_temperature,
                sample.hdd_temperature,
                sample.mainboard_temperature,
                sample.cpu_cores.len(),
                sample.disks.len()
            );
            for diagnostic in sample.diagnostics {
                println!("diagnostic={diagnostic}");
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(2));
    let call = Instant::now();
    let _ = sampler.sample("", "");
    println!("cached_call_us={}", call.elapsed().as_micros());
    let stop = Instant::now();
    println!("shutdown={:?}", sampler.shutdown());
    println!("shutdown_ms={}", stop.elapsed().as_millis());
}
