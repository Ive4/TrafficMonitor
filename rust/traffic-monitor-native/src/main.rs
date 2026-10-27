#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{ffi::OsString, io::Write, path::PathBuf, time::Duration};
use traffic_monitor_desktop_ui::DesktopBackend;
use traffic_monitor_native::app::{LaunchOptions, Runtime, resolve_paths};

struct Arguments {
    launch: LaunchOptions,
    headless: bool,
    samples: usize,
    interval: u64,
    help: bool,
    restarted: bool,
}
fn parse(arguments: impl Iterator<Item = OsString>) -> Result<Arguments, String> {
    let mut result = Arguments {
        launch: LaunchOptions::default(),
        headless: false,
        samples: 3,
        interval: 1000,
        help: false,
        restarted: false,
    };
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        let value = argument.to_str().ok_or("参数名必须是Unicode")?;
        match value {
            "--data-dir" => {
                result.launch.data_dir = Some(PathBuf::from(
                    arguments.next().ok_or("--data-dir 缺少目录")?,
                ))
            }
            "--resource-dir" => {
                result.launch.resource_dir = Some(PathBuf::from(
                    arguments.next().ok_or("--resource-dir 缺少目录")?,
                ))
            }
            "--plugin-dir" => {
                result.launch.plugin_dir = Some(PathBuf::from(
                    arguments.next().ok_or("--plugin-dir 缺少目录")?,
                ))
            }
            "--no-plugins" => result.launch.no_plugins = true,
            "--taskbar" => result.launch.taskbar = true,
            "--headless" => {
                result.headless = true;
                result.launch.test_mode = true;
            }
            "--exit-after-ms" => {
                let n = arguments
                    .next()
                    .ok_or("--exit-after-ms 缺少毫秒数")?
                    .to_str()
                    .ok_or("无效超时")?
                    .parse::<u32>()
                    .map_err(|_| "无效超时")?;
                if !(100..=600_000).contains(&n) {
                    return Err("超时应为100–600000毫秒".into());
                }
                result.launch.exit_after_ms = Some(n);
                result.launch.test_mode = true;
            }
            "--samples" => {
                result.samples = arguments
                    .next()
                    .ok_or("--samples 缺少次数")?
                    .to_str()
                    .ok_or("无效次数")?
                    .parse()
                    .map_err(|_| "无效次数")?;
                if !(1..=10_000).contains(&result.samples) {
                    return Err("采样次数应为1–10000".into());
                }
            }
            "--interval-ms" => {
                result.interval = arguments
                    .next()
                    .ok_or("--interval-ms 缺少毫秒数")?
                    .to_str()
                    .ok_or("无效间隔")?
                    .parse()
                    .map_err(|_| "无效间隔")?;
                if !(10..=30_000).contains(&result.interval) {
                    return Err("采样间隔应为10–30000毫秒".into());
                }
            }
            "--help" | "-h" => result.help = true,
            "--restart-manager" => result.restarted = true,
            _ => return Err(format!("未知参数：{value}")),
        }
    }
    if result.launch.test_mode && result.launch.data_dir.is_none() {
        return Err("验证运行必须通过 --data-dir 指定独立数据目录".into());
    }
    Ok(result)
}

struct Instance(windows_sys::Win32::Foundation::HANDLE);
// Quote UTF-16 directly: Windows paths may contain unpaired surrogate code units.
// Backslashes preceding a quote, including the closing quote, must be doubled.
fn quote_argument(argument: &std::ffi::OsStr, command: &mut Vec<u16>) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    command.push(b'"' as u16);
    let mut slashes = 0;
    for ch in argument.encode_wide() {
        if ch == 0 {
            return Err("restart argument contains a null character".into());
        }
        if ch == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        command.extend(std::iter::repeat_n(
            b'\\' as u16,
            if ch == b'"' as u16 {
                slashes * 2 + 1
            } else {
                slashes
            },
        ));
        command.push(ch);
        slashes = 0;
    }
    command.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    command.push(b'"' as u16);
    Ok(())
}
fn restart_command(
    arguments: &Arguments,
    original: &[OsString],
) -> Result<Option<Vec<u16>>, String> {
    if arguments.launch.test_mode || arguments.headless || arguments.help {
        return Ok(None);
    }
    let mut command = Vec::new();
    let mut is_path = false;
    for argument in original {
        if !command.is_empty() {
            command.push(b' ' as u16);
        }
        let path;
        let value = if is_path {
            // Restart Manager need not preserve the caller's working directory.
            path = std::path::absolute(argument).map_err(|e| e.to_string())?;
            path.as_os_str()
        } else {
            argument.as_os_str()
        };
        quote_argument(value, &mut command)?;
        is_path = !is_path
            && matches!(
                argument.to_str(),
                Some("--data-dir" | "--resource-dir" | "--plugin-dir")
            );
    }
    if !arguments.restarted {
        if !command.is_empty() {
            command.push(b' ' as u16);
        }
        quote_argument(std::ffi::OsStr::new("--restart-manager"), &mut command)?;
    }
    // Winbase.h RESTART_MAX_CMD_LINE is 1024, including the terminator.
    if command.len() >= 1024 {
        return Err("restart command exceeds the Windows 1024-character limit".into());
    }
    command.push(0);
    Ok(Some(command))
}
impl Instance {
    fn acquire(path: &std::path::Path) -> Result<Option<Self>, String> {
        use std::hash::{Hash, Hasher};
        use windows_sys::Win32::{
            Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
            System::Threading::CreateMutexW,
        };
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        path.to_string_lossy().to_lowercase().hash(&mut hash);
        let name: Vec<u16> = format!("Local\\TrafficMonitor.Rust.{:016x}", hash.finish())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

fn run() -> Result<(), String> {
    let original: Vec<_> = std::env::args_os().skip(1).collect();
    let arguments = parse(original.clone().into_iter())?;
    if arguments.help {
        println!(
            "TrafficMonitor (Rust) — Windows 10/11\n--data-dir PATH  --resource-dir PATH  --plugin-dir PATH  --no-plugins  --taskbar\nValidation: --headless --samples N --interval-ms N, or --exit-after-ms N (requires isolated --data-dir)"
        );
        return Ok(());
    }
    let paths = resolve_paths(&arguments.launch)?;
    paths.ensure_config_dir().map_err(|e| e.to_string())?;
    let ini = traffic_monitor_config::IniDocument::load(&paths.config_file).unwrap_or_default();
    if arguments.restarted && ini.get_bool("other", "exit_when_start_by_restart_manager", true) {
        return Ok(());
    }
    let _instance = match Instance::acquire(&paths.config_dir)? {
        Some(instance) => instance,
        None => {
            if ini.get_bool("other", "no_multistart_warning", false) {
                return Ok(());
            }
            return Err("这个数据目录已有TrafficMonitor正在运行".into());
        }
    };
    traffic_monitor_native::diagnostics::install(&paths.config_dir).map_err(|e| e.to_string())?;
    let log = std::path::absolute(&paths.error_log).map_err(|e| e.to_string())?;
    let panic_log = log.clone();
    let previous_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&panic_log)
        {
            let _ = writeln!(
                f,
                "{:?} thread={:?}: {info}",
                std::time::SystemTime::now(),
                std::thread::current().id()
            );
        }
        previous_panic_hook(info);
    }));
    let registration = restart_command(&arguments, &original).and_then(|command| {
        if let Some(command) = command {
            let result = unsafe {
                windows_sys::Win32::System::Recovery::RegisterApplicationRestart(
                    command.as_ptr(),
                    0,
                )
            };
            if result < 0 {
                return Err(format!(
                    "RegisterApplicationRestart failed: 0x{:08x}",
                    result as u32
                ));
            }
        }
        Ok(())
    });
    if let Err(error) = registration {
        eprintln!("{error}");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
        {
            let _ = writeln!(file, "{error}");
        }
    }
    let mut app = Runtime::load(arguments.launch)?;
    if arguments.headless {
        for index in 0..arguments.samples {
            if index > 0 {
                std::thread::sleep(Duration::from_millis(arguments.interval));
            }
            let dashboard = app.sample()?;
            println!(
                "{} | {}",
                dashboard.status,
                dashboard.tooltip.replace('\n', " | ")
            );
            for network in dashboard.networks.iter().filter(|n| n.selected) {
                println!("selected={} {}", network.id, network.name);
            }
            for note in dashboard.notices {
                println!("notice={note}");
            }
        }
        app.shutdown()
    } else {
        traffic_monitor_desktop_ui::run(Box::new(app))
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        if !std::env::args_os().any(|v| v == "--headless" || v == "--exit-after-ms") {
            let message: Vec<u16> = error.encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = "TrafficMonitor".encode_utf16().chain(Some(0)).collect();
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                        | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
                )
            };
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(a: &[&str]) -> Result<Arguments, String> {
        parse(a.iter().map(OsString::from))
    }
    #[test]
    fn validation_requires_scoped_data() {
        assert!(args(&["--headless"]).is_err());
        assert!(args(&["--exit-after-ms", "1000"]).is_err());
        assert!(args(&["--data-dir", "target/example", "--headless"]).is_ok());
    }
    #[test]
    fn invalid_arguments_are_errors() {
        assert!(args(&["--unknown"]).is_err());
        assert!(args(&["--samples", "0"]).is_err());
        assert!(args(&["--data-dir"]).is_err());
    }
    #[test]
    fn preserves_unicode_paths() {
        let a = args(&["--data-dir", "C:\\测试\\配置", "--exit-after-ms", "1000"]).unwrap();
        assert_eq!(a.launch.data_dir, Some(PathBuf::from("C:\\测试\\配置")));
    }
    fn split_windows_command(command: &[u16]) -> Vec<OsString> {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn CommandLineToArgvW(command: *const u16, count: *mut i32) -> *mut *mut u16;
        }
        let command: Vec<_> = std::ffi::OsStr::new("TrafficMonitor.exe ")
            .encode_wide()
            .chain(command.iter().copied())
            .collect();
        let mut count = 0;
        let argv = unsafe { CommandLineToArgvW(command.as_ptr(), &mut count) };
        assert!(!argv.is_null());
        let result = (1..count as usize)
            .map(|i| unsafe {
                let word = *argv.add(i);
                let mut len = 0;
                while *word.add(len) != 0 {
                    len += 1;
                }
                OsString::from_wide(std::slice::from_raw_parts(word, len))
            })
            .collect();
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(argv.cast());
        }
        result
    }
    #[test]
    fn restart_preserves_profile_arguments_and_windows_quoting() {
        let original: Vec<OsString> = [
            "--data-dir",
            "C:\\测试 空格\\",
            "--resource-dir",
            "C:\\a\"b\\",
            "--plugin-dir",
            "C:\\plug ins",
            "--no-plugins",
            "--taskbar",
            "--restart-manager",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        let parsed = parse(original.clone().into_iter()).unwrap();
        let command = restart_command(&parsed, &original).unwrap().unwrap();
        assert_eq!(split_windows_command(&command), original);
    }
    #[test]
    fn restart_is_disabled_for_all_validation_and_help_modes() {
        for original in [
            vec!["--headless", "--data-dir", "target/test"],
            vec!["--exit-after-ms", "1000", "--data-dir", "target/test"],
            vec!["--help"],
        ] {
            let original: Vec<OsString> = original.into_iter().map(OsString::from).collect();
            let parsed = parse(original.clone().into_iter()).unwrap();
            assert!(restart_command(&parsed, &original).unwrap().is_none());
        }
    }
    #[test]
    fn restart_resolves_relative_profiles_and_rejects_oversized_commands() {
        let original: Vec<OsString> = ["--data-dir", "relative profile", "--no-plugins"]
            .into_iter()
            .map(OsString::from)
            .collect();
        let parsed = parse(original.clone().into_iter()).unwrap();
        let decoded = split_windows_command(&restart_command(&parsed, &original).unwrap().unwrap());
        assert_eq!(
            PathBuf::from(&decoded[1]),
            std::env::current_dir().unwrap().join("relative profile")
        );
        assert_eq!(decoded.last().unwrap(), "--restart-manager");
        let original = vec![
            OsString::from("--data-dir"),
            OsString::from("x".repeat(1100)),
        ];
        let parsed = parse(original.clone().into_iter()).unwrap();
        assert!(restart_command(&parsed, &original).is_err());
    }
    #[test]
    fn quoting_preserves_empty_and_non_unicode_windows_arguments() {
        use std::os::windows::ffi::OsStringExt;
        let original = [
            OsString::new(),
            OsString::from_wide(&[0xd800, 0x20, 0x22, 0x5c]),
        ];
        let mut command = Vec::new();
        for value in &original {
            if !command.is_empty() {
                command.push(0x20);
            }
            quote_argument(value, &mut command).unwrap();
        }
        command.push(0);
        assert_eq!(split_windows_command(&command), original);
        assert!(quote_argument(std::ffi::OsStr::new("bad\0arg"), &mut Vec::new()).is_err());
    }
}
