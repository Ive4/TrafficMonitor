//! Platform-independent monitoring logic shared with the existing C++ application.

mod ffi;
mod network;

pub use network::{NetworkSample, calculate_network_sample};
