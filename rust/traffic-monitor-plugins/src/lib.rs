//! Native TrafficMonitor API 1–8 compatibility, without a C++ or MFC host.
//!
//! Plugins are trusted native code, not a sandbox. All calls and host callbacks must
//! execute on the creating UI thread. Never retain drawer pointers after drawing.
//! Plugin objects remain owned by their DLL; they are never deleted by this crate.
use std::collections::BTreeMap;
use std::path::PathBuf;

pub fn supports_api(version: i32) -> bool {
    (1..=8).contains(&version)
}

#[derive(Clone, Debug)]
pub struct HostInfo {
    pub version: String,
    pub language_id: u16,
    pub config_dir: PathBuf,
    pub main_window: usize,
    pub taskbar_window: usize,
    pub main_dpi: i32,
    pub taskbar_dpi: i32,
    pub theme_color: u32,
    /// ITrafficMonitor::MonitorItem order, NOT the display item enum:
    /// upload, download, CPU, memory, GPU usage, CPU/GPU/disk/board temperature,
    /// disk usage, CPU frequency (GHz), today's uploaded bytes, today's downloaded bytes.
    pub monitor_values: [f64; 13],
    /// Formatted values in the same MonitorItem order as `monitor_values`.
    pub main_value_strings: [String; 13],
    /// Formatted values in the same MonitorItem order as `monitor_values`.
    pub taskbar_value_strings: [String; 13],
    /// (key, section), matching GetStringRes(key, section).
    pub strings: BTreeMap<(String, String), String>,
    /// ExtendedInfoIndex values 0..=17. The config directory (17) is automatic.
    pub extended: BTreeMap<i32, String>,
}
impl Default for HostInfo {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            language_id: 0x409,
            config_dir: PathBuf::new(),
            main_window: 0,
            taskbar_window: 0,
            main_dpi: 96,
            taskbar_dpi: 96,
            theme_color: 0,
            monitor_values: [0.; 13],
            main_value_strings: std::array::from_fn(|_| String::new()),
            taskbar_value_strings: std::array::from_fn(|_| String::new()),
            strings: BTreeMap::new(),
            extended: BTreeMap::new(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MonitorInfo {
    pub up_speed: u64,
    pub down_speed: u64,
    pub cpu_usage: i32,
    pub memory_usage: i32,
    pub gpu_usage: i32,
    pub hdd_usage: i32,
    pub cpu_temperature: i32,
    pub gpu_temperature: i32,
    pub hdd_temperature: i32,
    pub main_board_temperature: i32,
    pub cpu_freq: i32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginCommand {
    pub index: usize,
    pub name: String,
    pub icon: usize,
    pub checked: bool,
}
#[derive(Clone, Debug)]
pub struct PluginInfo {
    pub id: String,
    pub path: PathBuf,
    pub enabled: bool,
    pub api_version: i32,
    pub properties: [String; 6],
    pub commands: Vec<PluginCommand>,
    pub error: Option<String>,
}
#[derive(Clone, Debug)]
pub struct PluginItem {
    pub id: String,
    pub plugin_id: String,
    pub name: String,
    pub label: String,
    pub value: String,
    pub sample: String,
    pub custom_draw: bool,
    pub width: i32,
    pub graph: Option<f32>,
    pub double_line: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionResult {
    Changed,
    Unchanged,
    NotProvided,
}
#[repr(i32)]
#[derive(Clone, Copy, Debug)]
pub enum MouseEventKind {
    Left = 0,
    Right = 1,
    Double = 2,
    WheelUp = 3,
    WheelDown = 4,
}
#[derive(Clone, Copy, Debug)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub x: i32,
    pub y: i32,
    pub window: usize,
    pub taskbar: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct KeyboardEvent {
    pub key: i32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub window: usize,
    pub taskbar: bool,
}

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::PluginManager;

#[cfg(test)]
mod tests {
    #[test]
    fn api_version_boundary() {
        for v in 1..=8 {
            assert!(super::supports_api(v), "API {v} must be supported");
        }
        for v in [-1, 0, 9, i32::MAX] {
            assert!(!super::supports_api(v));
        }
    }
    #[test]
    fn monitor_layout_matches_msvc() {
        assert_eq!(std::mem::size_of::<super::MonitorInfo>(), 56);
        assert_eq!(std::mem::offset_of!(super::MonitorInfo, cpu_freq), 48);
    }
}
