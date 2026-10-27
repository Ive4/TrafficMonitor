//! Read-only services probe. It never changes startup settings or opens applications.
use traffic_monitor_windows::services;
fn main() -> std::io::Result<()> {
    println!("theme={:?}", services::current_theme()?);
    let executable = std::env::current_exe()?;
    println!(
        "registry_startup_for_probe={:?}",
        services::auto_start_enabled(&executable, false)
    );
    println!(
        "scheduler_startup_for_probe={:?}",
        services::auto_start_enabled(&executable, true)
    );
    if let Some(argument) = std::env::args()
        .skip(1)
        .find(|a| a.starts_with("--update="))
    {
        let source = argument[9..]
            .parse::<u32>()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        match services::check_update("1.86", source) {
            Ok(update) => println!(
                "update current={} latest={} available={} url={} notes_chars={}",
                update.current,
                update.latest,
                update.available,
                update.url,
                update.notes.chars().count()
            ),
            Err(error) => println!("update_error={error}"),
        }
    }
    Ok(())
}
