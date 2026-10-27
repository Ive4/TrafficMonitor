use traffic_monitor_core::{NetworkSample, calculate_network_sample};

#[test]
fn first_sample_does_not_count_lifetime_traffic() {
    assert_eq!(
        calculate_network_sample(50_000, 20_000, 0, 0, 1000, false),
        NetworkSample::default()
    );
}

#[test]
fn normal_sample_produces_deltas_and_rates() {
    assert_eq!(
        calculate_network_sample(6000, 3000, 1000, 1000, 2000, false),
        NetworkSample {
            received_delta: 5000,
            sent_delta: 2000,
            receive_bytes_per_second: 2500,
            send_bytes_per_second: 1000,
        }
    );
}

#[test]
fn subsecond_interval_uses_actual_elapsed_time() {
    let result = calculate_network_sample(1500, 1250, 1000, 1000, 250, false);
    assert_eq!(result.receive_bytes_per_second, 2000);
    assert_eq!(result.send_bytes_per_second, 1000);
}

#[test]
fn integer_rates_round_down() {
    let result = calculate_network_sample(1001, 1002, 1000, 1000, 3000, false);
    assert_eq!(result.received_delta, 1);
    assert_eq!(result.sent_delta, 2);
    assert_eq!(result.receive_bytes_per_second, 0);
    assert_eq!(result.send_bytes_per_second, 0);
}

#[test]
fn changing_connection_discards_both_deltas() {
    assert_eq!(
        calculate_network_sample(6000, 3000, 1000, 1000, 1000, true),
        NetworkSample::default()
    );
}

#[test]
fn receive_counter_reset_discards_both_directions() {
    assert_eq!(
        calculate_network_sample(100, 3000, 1000, 1000, 1000, false),
        NetworkSample::default()
    );
}

#[test]
fn send_counter_reset_discards_both_directions() {
    assert_eq!(
        calculate_network_sample(6000, 100, 1000, 1000, 1000, false),
        NetworkSample::default()
    );
}

#[test]
fn legacy_32_bit_counter_wrap_is_treated_as_a_reset() {
    assert_eq!(
        calculate_network_sample(100, 3000, u32::MAX as u64 - 10, 1000, 1000, false),
        NetworkSample::default()
    );
}

#[test]
fn empty_current_counters_produce_no_traffic() {
    assert_eq!(
        calculate_network_sample(0, 0, 1000, 1000, 1000, false),
        NetworkSample::default()
    );
}

#[test]
fn a_single_zero_direction_does_not_invalidate_the_other() {
    let result = calculate_network_sample(0, 3000, 0, 1000, 1000, false);
    assert_eq!(result.sent_delta, 2000);
    assert_eq!(result.send_bytes_per_second, 2000);
    assert_eq!(result.received_delta, 0);
}

#[test]
fn unchanged_counters_produce_no_traffic() {
    assert_eq!(
        calculate_network_sample(1000, 1000, 1000, 1000, 1000, false),
        NetworkSample::default()
    );
}

#[test]
fn zero_elapsed_time_preserves_history_bytes_without_a_rate() {
    assert_eq!(
        calculate_network_sample(6000, 3000, 1000, 1000, 0, false),
        NetworkSample {
            received_delta: 5000,
            sent_delta: 2000,
            ..NetworkSample::default()
        }
    );
}

#[test]
fn negative_elapsed_time_preserves_history_bytes_without_a_rate() {
    let result = calculate_network_sample(6000, 3000, 1000, 1000, -1000, false);
    assert_eq!(result.received_delta, 5000);
    assert_eq!(result.sent_delta, 2000);
    assert_eq!(result.receive_bytes_per_second, 0);
    assert_eq!(result.send_bytes_per_second, 0);
}

#[test]
fn cumulative_counters_above_32_bits_do_not_truncate() {
    let baseline = u32::MAX as u64 + 5000;
    let result = calculate_network_sample(
        baseline + 6000,
        baseline + 3000,
        baseline,
        baseline,
        1000,
        false,
    );
    assert_eq!(result.received_delta, 6000);
    assert_eq!(result.sent_delta, 3000);
    assert_eq!(result.receive_bytes_per_second, 6000);
}

#[test]
fn scaling_uses_a_wide_intermediate_before_dividing() {
    let result = calculate_network_sample(u64::MAX, 1000, 1, 1, 1000, false);
    assert_eq!(result.received_delta, u64::MAX - 1);
    assert_eq!(result.receive_bytes_per_second, u64::MAX - 1);
}

#[test]
fn unrepresentable_rate_saturates_instead_of_wrapping() {
    let result = calculate_network_sample(u64::MAX, 2, 1, 1, 1, false);
    assert_eq!(result.receive_bytes_per_second, u64::MAX);
    assert_eq!(result.send_bytes_per_second, 1000);
}

#[test]
fn long_intervals_do_not_truncate_to_32_bits() {
    let result = calculate_network_sample(6000, 3000, 1000, 1000, i64::MAX, false);
    assert_eq!(result.received_delta, 5000);
    assert_eq!(result.receive_bytes_per_second, 0);
}
