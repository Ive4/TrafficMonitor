# Rust Win32 desktop frontend

`run(Box<dyn DesktopBackend>)` owns the UI thread and its message loop. The native runtime supplies real samples, configuration, network/history data and plugin services through `model.rs`. `desktop_demo` supplies isolated fixture data and never writes user configuration or startup settings.

Implemented surfaces include the draggable floating monitor, PNG alpha/BMP masks and skin item layout, notification icon/menu, an actual Explorer taskbar child, and Settings, Taskbar appearance, Advanced settings, History, Network, Appearance and Plugins windows. History uses a report ListView, native calendar, day/week/month/quarter/year ranges, and a chart with logarithmic scaling. Color/font/list editors use native dialogs and quoted CSV values. Changes dispatch typed backend actions; appearance and connection selections restore the current backend state. Startup status comes from the backend; an unknown status cannot be edited.

Fonts use points (DPI / 72), separately for main and taskbar windows. Skin rectangles and `draw_plugin` rectangles are left/top/right/bottom; the runtime converts plugin rectangles to the DLL's x/y/width/height convention. Plugin mouse coordinates are client coordinates of the provided window. Both text and custom-drawn plugin items receive mouse/keyboard input. Backend modal calls may reenter the message loop; `try_borrow` and queued actions prevent overlapping mutable backend borrows.

The application executable must embed a Windows 8/10 compatibility manifest for layered child windows. The fixture and tests embed `examples/desktop_demo.manifest`; the production executable embeds its own manifest. Even opaque taskbar displays use a layered child: an ordinary child can receive successful paint messages yet remain covered by Explorer's XAML composition. Child ordering, TaskbarCreated reconstruction, host geometry changes and automatic-hide visibility are handled; original task-button geometry is restored on removal. DPI entry points added after early Windows 10 are resolved dynamically.

GDI objects, icons, menus, tooltips, PNG surfaces and window classes are released on exit. Session queries call `checkpoint` without stopping services; a canceled logout continues sampling. A committed session end performs final shutdown. Resource ID 1 supplies the executable icon when available; shared icons are copied before ownership is taken.

## Verification

From the repository root (Cargo is `cargo`):

```powershell
cargo test -p traffic-monitor-desktop-ui --lib --locked --offline
cargo clippy -p traffic-monitor-desktop-ui --all-targets --locked --offline -- -D warnings
cargo test -p traffic-monitor-desktop-ui --lib --target i686-pc-windows-msvc --locked --offline
cargo clippy -p traffic-monitor-desktop-ui --all-targets --target i686-pc-windows-msvc --locked --offline -- -D warnings
```

Default tests do not manipulate the desktop. They cover DPI/layout, recovery-surface validation, independent chart switches and metric ranges, quoted CSV, localization, color propagation, actual PNG alpha pixels and actual DirectWrite color-emoji pixels. Explicit interactive acceptance (serial only) creates owned windows and briefly attaches a taskbar child:

```powershell
cargo test -p traffic-monitor-desktop-ui --lib --locked --offline -- --include-ignored --test-threads=1
cargo run -p traffic-monitor-desktop-ui --example desktop_demo --locked --offline -- --seconds 15 --taskbar --exercise
```

The ignored tests exercise nested message loops/cleanup, TaskbarCreated reconstruction/geometry restoration, and query-session/cancel/continued ticks/final shutdown. `examples/capture.ps1` captures fixture-owned panels and taskbar bounds, checks the taskbar child's process identity, and additionally uses physical screen capture. **Distinct pixel colors alone do not prove visible text**: inspect `taskbar-visible.png`. The verified fixture screenshot shows readable upload/download values; production screenshots are separately produced by the repository's native desktop capture script. Fixtures have a bounded lifetime and leave no process behind.

## Acceptance boundaries

Compilation and the local Windows 11 interactive checks are distinct from testing every Windows version or monitor arrangement. Physical cross-monitor dragging, all automatic-hide/orientation combinations, and an actual Explorer process restart still require target-device acceptance; the automated test sends TaskbarCreated without terminating Explorer. PNG-alpha floating text uses GDI+ to preserve transparency; colorful DirectWrite emoji applies to the ordinary main/taskbar text path. Backend tests cover persistence, startup operations, real sensor readings and plugin ABI separately; the fixture does not claim those external effects.
