TrafficMonitor — Rust edition, Windows 10/11

Run TrafficMonitor.exe, or TrafficMonitor-Updated.exe when provided alongside
an existing copy. Close the previous instance before opening the update.
The executable is written in Rust and does not require
the original MFC executable or traffic_monitor_core.dll. Windows system DLLs
and the Microsoft VC runtime used by the selected Rust MSVC toolchain still apply.

Keep skins/, language/ and res/ beside the executable. Configuration and history
follow global_cfg.ini portable_mode, the existing APPDATA/TrafficMonitor profile,
and directory write access. Use --data-dir PATH for an explicit isolated profile.
The old INI keys, language/skin files and KiB history format are retained.

Settings use seven tabs. In Display items, select and reorder the taskbar
metrics, then Apply. Recommended items selects upload, download, CPU and memory
without changing the floating window. Existing saved selections are retained.
Taskbar / Automatically fit content width removes the old default minimum
blank space; clearing all taskbar items hides the taskbar monitor entirely.

Optional native plugins belong in plugins/ and must match the executable's x64
or x86 architecture. These are trusted native DLLs. A plugin may have its own
runtime requirements independently of the Rust host.

Temperature readings depend on sensors exposed by the hardware and available
providers. Missing readings are shown as unavailable. See the migration report
for the exact tested scope and external device checks still needed.

Validation examples (always use a separate data directory):
  TrafficMonitor.exe --data-dir test-profile --headless --samples 3 --no-plugins
  TrafficMonitor.exe --data-dir test-profile --exit-after-ms 4000 --taskbar --no-plugins

The original LICENSE and resource authorship remain applicable.
