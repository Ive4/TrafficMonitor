#![cfg(windows)]
use std::path::{Path, PathBuf};
use std::ptr;
use traffic_monitor_plugins::*;
use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::*;
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn host() -> HostInfo {
    let mut h = HostInfo {
        language_id: 0x804,
        config_dir: std::env::temp_dir()
            .join(format!("TrafficMonitor-plugin-test-{}", std::process::id())),
        main_window: 123,
        taskbar_window: 456,
        main_dpi: 144,
        taskbar_dpi: 192,
        theme_color: 0x123456,
        ..Default::default()
    };
    h.monitor_values[2] = 42.5;
    h.main_value_strings[2] = "main CPU".into();
    h.taskbar_value_strings[2] = "taskbar CPU".into();
    h.strings
        .insert(("key".into(), "text".into()), "resource".into());
    h
}
struct Dc {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
}
impl Dc {
    fn new() -> Self {
        unsafe {
            let screen = GetDC(ptr::null_mut());
            let dc = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, 240, 80);
            ReleaseDC(ptr::null_mut(), screen);
            assert!(!dc.is_null() && !bitmap.is_null());
            let old = SelectObject(dc, bitmap);
            PatBlt(dc, 0, 0, 240, 80, BLACKNESS);
            Self { dc, bitmap, old }
        }
    }
}
impl Drop for Dc {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
#[test]
#[ignore = "Explicit trusted fixtures only; run verify-fixtures.ps1"]
fn rust_abi_fixture_all_versions_and_lifecycle() {
    let path = PathBuf::from(
        std::env::var("TM_RUST_PLUGIN")
            .expect("TM_RUST_PLUGIN must name the locally compiled Rust fixture"),
    )
    .canonicalize()
    .unwrap();
    let encoded = wide(path.to_str().unwrap());
    for version in 1..=8 {
        let module = unsafe {
            LoadLibraryExW(
                encoded.as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        };
        assert!(!module.is_null());
        let set: unsafe extern "C" fn(i32, i32) = unsafe {
            std::mem::transmute(GetProcAddress(module, c"FixtureSetApi".as_ptr().cast()).unwrap())
        };
        unsafe {
            set(version, 0);
        }
        let mut m = PluginManager::new(host());
        m.load(&path).unwrap();
        unsafe {
            FreeLibrary(module);
        }
        assert_eq!(m.plugins()[0].api_version, version);
        assert_eq!(m.plugins()[0].properties[0], "Rust");
        assert_eq!(
            m.configure(path.file_name().unwrap().to_str().unwrap(), 0)
                .unwrap(),
            OptionResult::Changed
        );
        let id = m.plugins()[0].id.clone();
        m.refresh(MonitorInfo {
            up_speed: 987654321,
            ..Default::default()
        })
        .unwrap();
        let items = m.items().unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].value.contains("up=987654321"));
        assert_eq!(
            items[0].graph,
            if version >= 6 { Some(0.625) } else { None }
        );
        assert_eq!(items[0].double_line, version >= 8);
        assert_eq!(
            m.tooltip(&id).unwrap(),
            if version >= 2 { "hint" } else { "" }
        );
        assert_eq!(
            m.mouse(
                "text",
                MouseEvent {
                    kind: MouseEventKind::Double,
                    x: 11,
                    y: 22,
                    window: 333,
                    taskbar: true
                }
            )
            .unwrap(),
            version >= 3
        );
        assert_eq!(
            m.keyboard(
                "text",
                KeyboardEvent {
                    key: 65,
                    ctrl: true,
                    shift: false,
                    alt: true,
                    window: 333,
                    taskbar: true
                }
            )
            .unwrap(),
            version >= 4
        );
        if version >= 5 {
            m.command(&id, 0, 333).unwrap();
            m.refresh(MonitorInfo::default()).unwrap();
            assert!(m.plugins()[0].commands[0].checked);
        } else {
            assert!(m.command(&id, 0, 333).is_err());
        }
        if version >= 7 {
            assert_eq!(
                m.take_notifications(),
                vec!["Rust initialized", "Rust command"]
            );
        }
        let dc = Dc::new();
        assert_eq!(
            m.item_width("draw", dc.dc as usize, false).unwrap(),
            if version >= 3 { 101 } else { 96 }
        );
        m.set_draw_colors("draw", 0x123456, 0xabcdef).unwrap();
        assert!(
            m.draw("draw", dc.dc as usize, [0, 0, 200, 60], true, false)
                .unwrap()
        );
        assert!(
            !m.draw("text", dc.dc as usize, [0, 0, 200, 60], false, false)
                .unwrap()
        );
        m.set_enabled(&id, false).unwrap();
        assert!(m.items().unwrap().is_empty());
        assert!(unsafe { GetModuleHandleW(encoded.as_ptr()) }.is_null());
        m.set_enabled(&id, true).unwrap();
        assert_eq!(m.items().unwrap().len(), 2);
        m.shutdown().unwrap();
        assert!(unsafe { GetModuleHandleW(encoded.as_ptr()) }.is_null());
        assert!(m.items().is_err());
    }
    for (version, mode) in [(0, 0), (9, 0), (8, 3), (8, 4), (8, 5)] {
        let module = unsafe { LoadLibraryW(encoded.as_ptr()) };
        assert!(!module.is_null());
        let set: unsafe extern "C" fn(i32, i32) = unsafe {
            std::mem::transmute(GetProcAddress(module, c"FixtureSetApi".as_ptr().cast()).unwrap())
        };
        unsafe {
            set(version, mode);
        }
        let mut m = PluginManager::new(host());
        assert!(m.load(&path).is_err());
        assert!(m.plugins()[0].error.is_some());
        assert!(m.items().unwrap().is_empty());
        unsafe {
            FreeLibrary(module);
        }
    }
}
#[test]
#[ignore = "Explicit repository-built PluginDemo only; run verify-fixtures.ps1"]
fn original_cpp_plugin_demo_load_refresh_draw_and_unload() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(if cfg!(target_arch = "x86") {
            "../../Bin/Release/plugins/PluginDemo.dll"
        } else {
            "../../Bin/x64/Release/plugins/PluginDemo.dll"
        })
        .canonicalize()
        .expect("build original PluginDemo first");
    let encoded = wide(path.to_str().unwrap());
    let mut m = PluginManager::new(host());
    m.load(&path).unwrap();
    assert_eq!(m.plugins()[0].api_version, 8);
    assert!(!m.plugins()[0].properties[0].is_empty());
    m.refresh(MonitorInfo::default()).unwrap();
    let items = m.items().unwrap();
    assert_eq!(items.len(), 4);
    assert!(items.iter().any(|i| !i.custom_draw && !i.value.is_empty()));
    assert_eq!(items.iter().filter(|i| i.custom_draw).count(), 2);
    let dc = Dc::new();
    for item in items {
        if item.custom_draw {
            assert!(m.item_width(&item.id, dc.dc as usize, false).unwrap() > 0);
            assert!(
                m.draw(&item.id, dc.dc as usize, [0, 0, 200, 60], true, false)
                    .unwrap()
            );
        }
    }
    // The original options callback opens a real modal dialog; never automate it in tests.
    assert!(m.command("PluginDemo.dll", 0, 0).is_err());
    m.shutdown().unwrap();
    assert!(unsafe { GetModuleHandleW(encoded.as_ptr()) }.is_null());
}
