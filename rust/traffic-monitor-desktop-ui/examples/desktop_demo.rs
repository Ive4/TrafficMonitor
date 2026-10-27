//! Explicit interactive acceptance fixture; never reads or writes user configuration.
use std::time::Instant;
use traffic_monitor_desktop_ui::*;
struct Demo {
    options: DesktopOptions,
    start: Instant,
    range: String,
    actions: usize,
    enabled: bool,
}
impl DesktopBackend for Demo {
    fn options(&self) -> DesktopOptions {
        self.options.clone()
    }
    fn sample(&mut self) -> Result<Dashboard, String> {
        let t = self.start.elapsed().as_secs_f64();
        let mut d = Dashboard {
            tooltip: "Isolated Rust desktop acceptance fixture\nNo user settings are written."
                .into(),
            status: format!("{} actions received", self.actions),
            history_summary: self.range.clone(),
            ..Default::default()
        };
        for (id, label, value, f) in [
            (
                "upload",
                "↑ Upload",
                format!("{:.1} MB/s", 1.2 + t.sin()),
                0.4,
            ),
            (
                "download",
                "↓ Download",
                format!("{:.1} MB/s", 3.2 + t.cos()),
                0.7,
            ),
            ("cpu", "CPU", format!("{:.0}%", 45.0 + 10.0 * t.sin()), 0.45),
            ("memory", "Memory", "8.2 / 16 GB".into(), 0.51),
        ] {
            d.items.push(DisplayValue {
                id: id.into(),
                label: label.into(),
                value,
                fraction: Some(f),
                history: (0..30)
                    .map(|i| ((i as f64 + t) * 0.3).sin() + 1.2)
                    .collect(),
                ..Default::default()
            });
        }
        d.taskbar_items = d.items.clone();
        d.networks = vec![NetworkInfo {
            id: "demo-ethernet".into(),
            name: "Isolated sample Ethernet".into(),
            selected: true,
            details: vec![
                ("IPv4".into(), "192.0.2.10 (documentation address)".into()),
                ("Status".into(), "Fixture data".into()),
            ],
        }];
        d.history = (1..=20)
            .map(|day| HistoryRow {
                date: format!("2026-10-{day:02}"),
                upload: format!("{} MB", day * 10),
                download: format!("{} MB", day * 40),
                total: format!("{} MB", day * 50),
                mixed: String::new(),
                total_bytes: day * 50 * 1024 * 1024,
            })
            .collect();
        d.plugins = vec![PluginInfo {
            id: "demo".into(),
            name: "Fixture plugin".into(),
            version: "1.0".into(),
            enabled: self.enabled,
            description: "Exercises plugin management actions without loading a DLL".into(),
            commands: vec![(1, "Demo command".into())],
        }];
        d.settings = vec![
            SettingField {
                key: "general/example_bool".into(),
                label: "Example Boolean setting".into(),
                value: "true".into(),
                kind: SettingKind::Bool,
                help: "Fixture only".into(),
            },
            SettingField {
                key: "general/example_text".into(),
                label: "Example text setting".into(),
                value: "Fixture".into(),
                kind: SettingKind::Text,
                help: String::new(),
            },
        ];
        d.skins = vec![Choice {
            id: "default".into(),
            name: "Built-in layout".into(),
        }];
        d.languages = vec![Choice {
            id: "en-us".into(),
            name: "English".into(),
        }];
        Ok(d)
    }
    fn dispatch(&mut self, a: DesktopAction) -> Result<(), String> {
        self.actions += 1;
        println!("ACTION {a:?}");
        match a {
            DesktopAction::ApplyOptions(o) => self.options = o,
            DesktopAction::ApplyPreferences { options, .. } => self.options = options,
            DesktopAction::PositionChanged { x, y } => {
                self.options.x = x;
                self.options.y = y
            }
            DesktopAction::HistoryRange { from, to, grouping } => {
                self.range = format!("Requested range: {from} – {to}, {grouping:?}")
            }
            DesktopAction::PluginEnabled { enabled, .. } => self.enabled = enabled,
            _ => {}
        }
        Ok(())
    }
    fn attach_windows(&mut self, main: usize, taskbar: Option<usize>) {
        let _ = (main, taskbar);
    }
    fn shutdown(&mut self) -> Result<(), String> {
        println!("SHUTDOWN actions={}", self.actions);
        Ok(())
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let seconds = args
        .windows(2)
        .find(|a| a[0] == "--seconds")
        .and_then(|a| a[1].parse::<u32>().ok())
        .unwrap_or(15);
    let mut options = DesktopOptions {
        title: "TrafficMonitor isolated UI fixture".into(),
        show_taskbar: args.iter().any(|a| a == "--taskbar"),
        exit_after_ms: Some(seconds.saturating_mul(1000)),
        ..Default::default()
    };
    options.taskbar_style.visible_items = vec![
        "upload".into(),
        "download".into(),
        "cpu".into(),
        "memory".into(),
    ];
    #[cfg(windows)]
    let worker = if args.iter().any(|a| a == "--exercise") {
        Some(std::thread::spawn(move || {
            use std::{ptr::null, thread::sleep, time::Duration};
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let title: Vec<u16> = "TrafficMonitor isolated UI fixture"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut window = std::ptr::null_mut();
            for _ in 0..100 {
                unsafe {
                    window = FindWindowW(null(), title.as_ptr());
                }
                if !window.is_null() {
                    break;
                }
                sleep(Duration::from_millis(50));
            }
            assert!(!window.is_null(), "Fixture main window must be created");
            for id in [
                100, 200, 114, 200, 101, 204, 102, 208, 103, 212, 104, 210, 211, 105, 105,
            ] {
                sleep(Duration::from_millis(300));
                unsafe {
                    PostMessageW(window, WM_COMMAND, id, 0);
                }
            }
        }))
    } else {
        None
    };
    let result = run(Box::new(Demo {
        options,
        start: Instant::now(),
        range: "20 fixture days, 10.25 GB total".into(),
        actions: 0,
        enabled: true,
    }));
    #[cfg(windows)]
    if let Some(worker) = worker {
        worker.join().expect("UI exercise thread");
    }
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
