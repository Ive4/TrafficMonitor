#pragma once

#include <stdint.h>

#ifdef _WIN32
#define TM_CORE_API __declspec(dllimport)
#define TM_CORE_CALL __cdecl
#else
#define TM_CORE_API
#define TM_CORE_CALL
#endif

#ifdef __cplusplus
extern "C" {
#endif

// Layout and field order must match Rust's repr(C) NetworkSample.
typedef struct TmNetworkSample {
    uint64_t received_delta;
    uint64_t sent_delta;
    uint64_t receive_bytes_per_second;
    uint64_t send_bytes_per_second;
} TmNetworkSample;

// Nonzero connection_changed resets both directions. Nonpositive elapsed_ms
// preserves valid deltas but produces zero rates. Rates saturate at UINT64_MAX.
TM_CORE_API TmNetworkSample TM_CORE_CALL tm_calculate_network_sample(
    uint64_t received,
    uint64_t sent,
    uint64_t previous_received,
    uint64_t previous_sent,
    int64_t elapsed_ms,
    uint32_t connection_changed);

#ifdef __cplusplus
}
#endif

#undef TM_CORE_API
#undef TM_CORE_CALL
