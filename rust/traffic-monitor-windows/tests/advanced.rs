use traffic_monitor_windows::advanced::*;

#[test]
fn owned_snapshot_has_explicit_missing_values() {
    let snapshot = HardwareSnapshot::default();
    assert!(snapshot.cpu_frequency_mhz.is_none());
    assert!(snapshot.gpu_usage_percent.is_none());
    assert!(snapshot.disk_usage_percent.is_none());
}

#[cfg(windows)]
#[test]
fn network_details_share_counter_luids_and_have_no_duplicate_identity() {
    let counters = traffic_monitor_windows::network::snapshot().unwrap();
    let details = network_details().unwrap();
    assert!(!details.is_empty());
    let mut ids = std::collections::HashSet::new();
    for detail in details {
        assert_ne!(detail.id, 0);
        assert!(ids.insert(detail.id));
        assert!(counters.iter().any(|row| row.id == detail.id));
        for address in detail.addresses {
            assert!(address.contains('/'));
        }
    }
}
