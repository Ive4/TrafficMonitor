use std::process::ExitCode;

use traffic_monitor_shell::{probe_hidden_child, probe_taskbars};

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let hidden_child = match arguments.as_slice() {
        [] => false,
        [argument] if argument == "--hidden-child" => true,
        _ => {
            eprintln!("usage: taskbar_probe [--hidden-child]");
            return ExitCode::from(2);
        }
    };
    match probe_taskbars() {
        Ok(hosts) => {
            for host in hosts {
                println!("{host:#?}");
                if hidden_child {
                    match probe_hidden_child(&host) {
                        Ok(report) => println!("hidden child lifecycle: {report:#?}"),
                        Err(error) => {
                            eprintln!("hidden child lifecycle failed: {error}");
                            return ExitCode::FAILURE;
                        }
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("taskbar probe unavailable: {error}");
            ExitCode::FAILURE
        }
    }
}
