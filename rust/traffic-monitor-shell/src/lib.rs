//! Read-only taskbar discovery and an opt-in hidden child-window lifecycle probe.

mod layout;
mod lifecycle;

pub use layout::{
    Alignment, LayoutError, LayoutRequest, Orientation, Rect, Size, layout_panel,
    taskbar_orientation,
};
pub use lifecycle::LifecycleState;

#[cfg(windows)]
mod windows;

/// Identifies a window at the time of probing, without transferring ownership.
///
/// Explorer can destroy or reuse the handle after the probe returns. Treat this
/// value as diagnostic data, not a lifetime guarantee or a native handle to own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowId(usize);

impl WindowId {
    pub fn value(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskbarKind {
    Primary,
    Secondary,
}

/// A read-only snapshot obtained in a per-monitor-aware thread context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowSnapshot {
    pub window: WindowId,
    pub class_name: String,
    pub process_id: u32,
    pub thread_id: u32,
    pub screen_rect: Rect,
    pub client_rect: Rect,
    pub client_screen_rect: Rect,
    /// Effective DPI of this window, as reported by GetDpiForWindow.
    pub dpi: u32,
}

/// Observed Explorer topology, not an assertion of free space or OS version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskbarHost {
    pub kind: TaskbarKind,
    pub taskbar: WindowSnapshot,
    pub classical_container: Option<WindowSnapshot>,
    pub task_buttons: Option<WindowSnapshot>,
    pub notification_area: Option<WindowSnapshot>,
    pub start_button: Option<WindowSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleReport {
    /// Diagnostic only: this window has already been destroyed on success.
    pub child_window: WindowId,
    pub parent_window: WindowId,
    pub states: Vec<LifecycleState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeError {
    UnsupportedPlatform,
    /// No primary or secondary taskbar was observed on the calling desktop.
    ExplorerUnavailable,
    /// Required thread/window DPI APIs are absent (pre-1607 Windows 10).
    UnsupportedDpiApi(&'static str),
    Win32 {
        operation: &'static str,
        code: u32,
    },
    HostChanged,
    UnexpectedChildState(&'static str),
    ResourceLimit,
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedPlatform => formatter.write_str("taskbar probing requires Windows"),
            Self::ExplorerUnavailable => formatter.write_str(
                "no Explorer taskbar on this desktop; Explorer may be stopped or this may be a CI/service/noninteractive session",
            ),
            Self::UnsupportedDpiApi(api) => write!(formatter, "required DPI API is unavailable: {api}"),
            Self::Win32 { operation, code } => write!(formatter, "{operation} failed (Win32 error {code})"),
            Self::HostChanged => formatter.write_str("Explorer host changed since discovery; probe again"),
            Self::UnexpectedChildState(reason) => write!(formatter, "hidden child verification failed: {reason}"),
            Self::ResourceLimit => formatter.write_str("taskbar enumeration exceeded available memory"),
        }
    }
}

impl std::error::Error for ProbeError {}

/// Reads taskbar hosts and their relevant child-window geometry/DPI.
///
/// Does not create windows, reserve space, or change Explorer geometry. A scoped
/// thread DPI context is restored before returning. Missing child classes are
/// represented by None; no taskbar at all is an explicit diagnostic.
pub fn probe_taskbars() -> Result<Vec<TaskbarHost>, ProbeError> {
    #[cfg(windows)]
    return windows::probe_taskbars();
    #[cfg(not(windows))]
    Err(ProbeError::UnsupportedPlatform)
}

/// Creates, verifies, destroys a hidden child and unregisters its private class.
///
/// This is deliberately separate from read-only discovery. Call only explicitly
/// from an interactive Windows thread. All ownership remains on that thread;
/// no window, timer, message loop or background thread survives this call.
pub fn probe_hidden_child(host: &TaskbarHost) -> Result<LifecycleReport, ProbeError> {
    #[cfg(windows)]
    return windows::probe_hidden_child(host);
    #[cfg(not(windows))]
    {
        let _ = host;
        Err(ProbeError::UnsupportedPlatform)
    }
}
