use crate::{NetworkSample, calculate_network_sample};

// All values cross the boundary by value; no allocations or unwinding are needed.
#[unsafe(no_mangle)]
pub extern "C" fn tm_calculate_network_sample(
    received: u64,
    sent: u64,
    previous_received: u64,
    previous_sent: u64,
    elapsed_ms: i64,
    connection_changed: u32,
) -> NetworkSample {
    calculate_network_sample(
        received,
        sent,
        previous_received,
        previous_sent,
        elapsed_ms,
        connection_changed != 0,
    )
}
