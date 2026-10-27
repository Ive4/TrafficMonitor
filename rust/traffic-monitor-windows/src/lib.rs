//! Read-only Windows monitoring adapters.

pub mod advanced;
pub mod network;
pub mod plugin_updates;
pub mod sensors;
pub mod services;
#[cfg(windows)]
mod startup_identity;
pub mod system;
