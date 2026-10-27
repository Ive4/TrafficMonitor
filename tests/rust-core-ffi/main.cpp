#include "TrafficMonitorCore.h"

#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <limits>

static_assert(sizeof(TmNetworkSample) == 32);
static_assert(offsetof(TmNetworkSample, received_delta) == 0);
static_assert(offsetof(TmNetworkSample, sent_delta) == 8);
static_assert(offsetof(TmNetworkSample, receive_bytes_per_second) == 16);
static_assert(offsetof(TmNetworkSample, send_bytes_per_second) == 24);

static int failures = 0;

static void Check(bool condition, const char* message)
{
    if (!condition)
    {
        std::fprintf(stderr, "FAIL: %s\n", message);
        ++failures;
    }
}

int main()
{
    const auto normal = tm_calculate_network_sample(6000, 3000, 1000, 1000, 2000, 0);
    Check(normal.received_delta == 5000 && normal.sent_delta == 2000, "history deltas");
    Check(normal.receive_bytes_per_second == 2500 && normal.send_bytes_per_second == 1000, "display rates");

    const auto reset = tm_calculate_network_sample(6000, 3000, 1000, 1000, 1000, 2);
    Check(reset.received_delta == 0 && reset.sent_delta == 0, "all nonzero reset flags");
    Check(reset.receive_bytes_per_second == 0 && reset.send_bytes_per_second == 0, "reset display rates");

    const auto rollback = tm_calculate_network_sample(6000, 100, 1000, 1000, 1000, 0);
    Check(rollback.received_delta == 0 && rollback.sent_delta == 0, "rollback resets both directions");

    const auto zero_time = tm_calculate_network_sample(6000, 3000, 1000, 1000, 0, 0);
    Check(zero_time.received_delta == 5000 && zero_time.sent_delta == 2000, "zero time preserves history");
    Check(zero_time.receive_bytes_per_second == 0 && zero_time.send_bytes_per_second == 0, "zero time produces no rate");

    const auto negative_time = tm_calculate_network_sample(6000, 3000, 1000, 1000, -1, 0);
    Check(negative_time.received_delta == 5000 && negative_time.receive_bytes_per_second == 0, "signed elapsed time ABI");

    const auto maximum = std::numeric_limits<std::uint64_t>::max();
    const auto wide = tm_calculate_network_sample(maximum, maximum, 1, 2, 1000, 0);
    Check(wide.received_delta == maximum - 1 && wide.sent_delta == maximum - 2, "64-bit counter ABI");
    Check(wide.receive_bytes_per_second == maximum - 1, "wide intermediate multiplication");

    const auto saturated = tm_calculate_network_sample(maximum, 2, 1, 1, 1, 0);
    Check(saturated.receive_bytes_per_second == maximum && saturated.send_bytes_per_second == 1000, "saturated rate");

    if (failures != 0)
        return 1;

    std::puts("Rust core C++ ABI checks passed (11 checks).");
    return 0;
}
