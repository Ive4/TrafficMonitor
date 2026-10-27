/// Byte deltas for history accounting and integer byte-per-second display rates.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NetworkSample {
    pub received_delta: u64,
    pub sent_delta: u64,
    pub receive_bytes_per_second: u64,
    pub send_bytes_per_second: u64,
}

pub fn calculate_network_sample(
    received: u64,
    sent: u64,
    previous_received: u64,
    previous_sent: u64,
    elapsed_ms: i64,
    connection_changed: bool,
) -> NetworkSample {
    if connection_changed
        || (received == 0 && sent == 0)
        || (previous_received == 0 && previous_sent == 0)
        || received < previous_received
        || sent < previous_sent
    {
        return NetworkSample::default();
    }

    let received_delta = received - previous_received;
    let sent_delta = sent - previous_sent;
    let rate = |delta: u64| {
        if elapsed_ms <= 0 {
            return 0;
        }
        let scaled = u128::from(delta) * 1000 / elapsed_ms as u128;
        scaled.min(u128::from(u64::MAX)) as u64
    };

    NetworkSample {
        received_delta,
        sent_delta,
        receive_bytes_per_second: rate(received_delta),
        send_bytes_per_second: rate(sent_delta),
    }
}
