use traffic_monitor_history::{Date, HistoryDocument};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut history = HistoryDocument::parse(include_str!("../tests/fixtures/history_legacy.txt"))?;
    let today = Date::new(2026, 10, 27)?;
    let report = history.normalize(today)?;
    assert!(report.discarded_future.is_empty());
    let output = history.to_legacy_compatible_string()?;
    assert_eq!(HistoryDocument::parse(&output)?, history);
    print!("{output}");
    Ok(())
}
