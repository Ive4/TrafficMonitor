use traffic_monitor_windows::advanced::{HardwareSampler, network_details};
fn main() -> std::io::Result<()> {
    let mut hardware = HardwareSampler::new();
    for iteration in 0..3 {
        if iteration > 0 {
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        println!("hardware[{iteration}]: {:#?}", hardware.sample(""));
    }
    for detail in network_details()? {
        println!("adapter: {detail:#?}");
    }
    Ok(())
}
