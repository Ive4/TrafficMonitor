//! Native Win32 floating monitor, taskbar, tray and accessible management windows.
mod model;
pub use model::*;
#[cfg(windows)]
mod color_text;
mod layout;
mod localization;
mod settings_tabs;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::run;
#[cfg(not(windows))]
pub fn run(_backend: Box<dyn DesktopBackend>) -> Result<(), String> {
    Err("Desktop frontend requires Windows 10 or later".into())
}
