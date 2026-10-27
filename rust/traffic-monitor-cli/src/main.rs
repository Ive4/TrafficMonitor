mod args;

use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::time::{Duration, Instant};

use args::{Command, parse_command};
use traffic_monitor_core::calculate_network_sample;
use traffic_monitor_history::HistoryDocument;
use traffic_monitor_windows::{network, system};

fn main() {
    if let Err(error) = run() {
        eprintln!("TrafficMonitor: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let command = parse_command(&std::env::args_os().skip(1).collect::<Vec<_>>())
        .map_err(io::Error::other)?;
    match command {
        Command::Help => println!(
            "TrafficMonitor (Windows 10/11)\n\
             Usage:\n  traffic-monitor sample [count=3] [interval-ms=1000]\n  \
             traffic-monitor taskbar-probe\n  traffic-monitor history-check <path>\n\
             All commands are read-only. Sampling reports interfaces separately."
        ),
        Command::Sample { count, interval_ms } => sample(count, interval_ms)?,
        Command::TaskbarProbe => {
            let hosts = traffic_monitor_shell::probe_taskbars()
                .map_err(|error| io::Error::other(format!("{error:?}")))?;
            for host in hosts {
                println!("{host:?}");
            }
        }
        Command::HistoryCheck(path) => {
            let bytes = std::fs::read(&path)?;
            let history = HistoryDocument::parse_bytes(&bytes)
                .map_err(|error| io::Error::other(format!("{error:?}")))?;
            println!("History valid: {} records.", history.records().len());
        }
    }
    Ok(())
}

fn sample(count: u32, interval_ms: u64) -> io::Result<()> {
    let mut previous = counters(&network::snapshot()?);
    let mut previous_cpu = system::cpu_times()?;
    let mut sampled_at = Instant::now();

    for number in 1..=count {
        std::thread::sleep(Duration::from_millis(interval_ms));
        let adapters = network::snapshot()?;
        let now = Instant::now();
        let elapsed_ms = now
            .duration_since(sampled_at)
            .as_millis()
            .try_into()
            .unwrap_or(i64::MAX);
        let cpu = system::cpu_times()?;
        let cpu_usage = system::cpu_usage(previous_cpu, cpu)
            .map(|usage| format!("{usage:.1}%"))
            .unwrap_or_else(|| "unavailable".into());
        let memory = system::memory_status()?;
        println!(
            "Sample {number}/{count}: CPU {cpu_usage}, memory {}/{} MiB ({}%)",
            memory.total_bytes.saturating_sub(memory.available_bytes) / (1024 * 1024),
            memory.total_bytes / (1024 * 1024),
            memory.load_percent,
        );

        // IP Helper also reports virtual/filter interfaces. Report identities
        // individually: summing all operational rows can double-count traffic.
        for adapter in adapters
            .iter()
            .filter(|adapter| adapter.operational && !adapter.is_loopback)
        {
            let (received, sent) = previous.get(&adapter.id).copied().unwrap_or_default();
            let result = calculate_network_sample(
                adapter.received_bytes,
                adapter.sent_bytes,
                received,
                sent,
                elapsed_ms,
                false,
            );
            println!(
                "  [{}] {}: down {:.2} KiB/s, up {:.2} KiB/s",
                adapter.interface_index,
                adapter.alias,
                result.receive_bytes_per_second as f64 / 1024.0,
                result.send_bytes_per_second as f64 / 1024.0,
            );
        }
        previous = counters(&adapters);
        previous_cpu = cpu;
        sampled_at = now;
    }
    Ok(())
}

fn counters(adapters: &[network::NetworkAdapter]) -> HashMap<u64, (u64, u64)> {
    adapters
        .iter()
        .filter(|adapter| adapter.operational && !adapter.is_loopback)
        .map(|adapter| (adapter.id, (adapter.received_bytes, adapter.sent_bytes)))
        .collect()
}
