//! User-triggered shell, startup and bounded HTTPS update services.
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct ThemeInfo {
    pub dark: bool,
    pub system_dark: bool,
    /// COLORREF (0x00BBGGRR), matching the plugin and GDI interfaces.
    pub color: u32,
}
#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    pub url: String,
    pub notes: String,
}

/// Shared bounded transport for trusted GitHub/Gitee raw update manifests.
pub(crate) fn fetch_https(url: &str) -> io::Result<String> {
    #[cfg(windows)]
    {
        platform::https_get(url)
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows HTTPS service unavailable",
        ))
    }
}
#[cfg(windows)]
pub use platform::{
    auto_start_enabled, check_update, current_theme, launch_application, open_path,
    open_task_manager, open_url, set_auto_start,
};
#[cfg(not(windows))]
mod unsupported {
    use super::*;
    fn unavailable<T>() -> io::Result<T> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows service unavailable on this platform",
        ))
    }
    pub fn current_theme() -> io::Result<ThemeInfo> {
        unavailable()
    }
    pub fn set_auto_start(_executable: &Path, _enabled: bool, _scheduler: bool) -> io::Result<()> {
        unavailable()
    }
    pub fn auto_start_enabled(_executable: &Path, _scheduler: bool) -> io::Result<bool> {
        unavailable()
    }
    pub fn open_path(_path: &Path) -> io::Result<()> {
        unavailable()
    }
    pub fn open_url(_url: &str) -> io::Result<()> {
        unavailable()
    }
    pub fn open_task_manager() -> io::Result<()> {
        unavailable()
    }
    pub fn launch_application(_path: &Path) -> io::Result<()> {
        unavailable()
    }
    pub fn check_update(_current: &str, _source: u32) -> io::Result<UpdateInfo> {
        unavailable()
    }
}
#[cfg(not(windows))]
pub use unsupported::*;

const MAX_RESPONSE: usize = 1024 * 1024;
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
#[derive(Debug, PartialEq, Eq)]
struct Version {
    numbers: Vec<u64>,
    pre: Option<String>,
}
fn version(value: &str) -> io::Result<Version> {
    let value = value.strip_prefix(['v', 'V']).unwrap_or(value);
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-')
    {
        return Err(invalid("invalid version string"));
    }
    let end = value
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(value.len());
    let main = &value[..end];
    let numbers = main
        .split('.')
        .map(|part| {
            part.parse::<u64>()
                .map_err(|_| invalid("invalid numeric version component"))
        })
        .collect::<io::Result<Vec<_>>>()?;
    if numbers.len() > 8 {
        return Err(invalid("too many version components"));
    }
    let pre = if end < value.len() {
        let pre = value[end..].strip_prefix('-').unwrap_or(&value[end..]);
        if pre.is_empty() || pre.split('.').any(str::is_empty) {
            return Err(invalid("invalid prerelease version"));
        }
        Some(pre.into())
    } else {
        None
    };
    Ok(Version { numbers, pre })
}
fn newer(current: &str, latest: &str) -> io::Result<bool> {
    use std::cmp::Ordering;
    let current = version(current)?;
    let latest = version(latest)?;
    for i in 0..current.numbers.len().max(latest.numbers.len()) {
        match latest
            .numbers
            .get(i)
            .copied()
            .unwrap_or(0)
            .cmp(&current.numbers.get(i).copied().unwrap_or(0))
        {
            Ordering::Greater => return Ok(true),
            Ordering::Less => return Ok(false),
            Ordering::Equal => {}
        }
    }
    match (&current.pre, &latest.pre) {
        (Some(_), None) => Ok(true),
        (None, Some(_)) | (None, None) => Ok(false),
        (Some(a), Some(b)) => {
            let mut a = a.split('.');
            let mut b = b.split('.');
            loop {
                match (a.next(), b.next()) {
                    (None, None) => return Ok(false),
                    (None, Some(_)) => return Ok(true),
                    (Some(_), None) => return Ok(false),
                    (Some(a), Some(b)) => {
                        let cmp = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(a), Ok(b)) => b.cmp(&a),
                            (Ok(_), Err(_)) => Ordering::Greater,
                            (Err(_), Ok(_)) => Ordering::Less,
                            _ => b.cmp(a),
                        };
                        if cmp != Ordering::Equal {
                            return Ok(cmp == Ordering::Greater);
                        }
                    }
                }
            }
        }
    }
}
fn xml_fields(xml: &str) -> io::Result<BTreeMap<String, Vec<String>>> {
    use quick_xml::events::Event;
    if xml.len() > MAX_RESPONSE {
        return Err(invalid("XML response exceeds 1 MiB"));
    }
    let mut reader = quick_xml::Reader::from_str(xml.trim_start_matches('\u{feff}'));
    let mut names = Vec::<String>::new();
    let mut texts = Vec::<String>::new();
    let mut fields = BTreeMap::<String, Vec<String>>::new();
    let mut roots = 0;
    let mut nodes = 0;
    loop {
        match reader
            .read_event()
            .map_err(|e| invalid(format!("invalid XML: {e}")))?
        {
            Event::Start(e) => {
                nodes += 1;
                if names.is_empty() {
                    roots += 1;
                }
                names.push(
                    String::from_utf8(e.local_name().as_ref().to_vec())
                        .map_err(|e| invalid(e.to_string()))?,
                );
                texts.push(String::new());
                if names.len() > 32 || nodes > 4096 {
                    return Err(invalid("XML nesting/node limit exceeded"));
                }
            }
            Event::Empty(e) => {
                nodes += 1;
                if nodes > 4096 || names.len() >= 32 {
                    return Err(invalid("XML nesting/node limit exceeded"));
                }
                if names.is_empty() {
                    roots += 1;
                }
                names.push(
                    String::from_utf8(e.local_name().as_ref().to_vec())
                        .map_err(|e| invalid(e.to_string()))?,
                );
                fields
                    .entry(names.join("/"))
                    .or_default()
                    .push(String::new());
                names.pop();
            }
            Event::Text(e) => {
                let text = e.decode().map_err(|e| invalid(e.to_string()))?;
                if let Some(value) = texts.last_mut() {
                    value.push_str(&text);
                } else if !text.trim().is_empty() {
                    return Err(invalid("text outside XML document"));
                }
            }
            Event::CData(e) => {
                if let Some(value) = texts.last_mut() {
                    value.push_str(&e.decode().map_err(|e| invalid(e.to_string()))?);
                } else {
                    return Err(invalid("CDATA outside XML document"));
                }
            }
            Event::GeneralRef(e) => {
                let reference = e.decode().map_err(|e| invalid(e.to_string()))?;
                let text =
                    if let Some(ch) = e.resolve_char_ref().map_err(|e| invalid(e.to_string()))? {
                        ch.to_string()
                    } else {
                        match reference.as_ref() {
                            "amp" => "&",
                            "lt" => "<",
                            "gt" => ">",
                            "quot" => "\"",
                            "apos" => "'",
                            _ => return Err(invalid("unsupported XML entity")),
                        }
                        .to_owned()
                    };
                texts
                    .last_mut()
                    .ok_or_else(|| invalid("entity outside XML document"))?
                    .push_str(&text);
            }
            Event::End(_) => {
                let value = texts.pop().ok_or_else(|| invalid("unbalanced XML"))?;
                fields
                    .entry(names.join("/"))
                    .or_default()
                    .push(value.trim().to_owned());
                names.pop();
            }
            Event::DocType(_) => return Err(invalid("DTD is not supported in service responses")),
            Event::Eof => break,
            _ => {}
        }
    }
    if roots != 1 || !names.is_empty() {
        return Err(invalid("expected one complete XML document"));
    }
    Ok(fields)
}
fn field<'a>(fields: &'a BTreeMap<String, Vec<String>>, path: &str) -> io::Result<Option<&'a str>> {
    match fields.get(path) {
        None => Ok(None),
        Some(values) if values.len() == 1 => Ok(Some(&values[0])),
        Some(_) => Err(invalid(format!("duplicate XML field {path}"))),
    }
}
fn required<'a>(fields: &'a BTreeMap<String, Vec<String>>, path: &str) -> io::Result<&'a str> {
    field(fields, path)?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(format!("missing XML field {path}")))
}
fn https_parts(url: &str) -> io::Result<(&str, &str)> {
    if url.len() > 4096
        || url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return Err(invalid("invalid HTTPS URL"));
    }
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| invalid("HTTPS is required"))?;
    let (host, path) = rest
        .split_once('/')
        .ok_or_else(|| invalid("URL path is missing"))?;
    if host.is_empty()
        || !host
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-')
    {
        return Err(invalid("invalid HTTPS host"));
    }
    Ok((host, path))
}
fn trusted_release_url(url: &str) -> io::Result<()> {
    let (host, path) = https_parts(url)?;
    if !matches!(host, "github.com" | "gitee.com")
        || !path.starts_with("zhongyang219/TrafficMonitor/releases/")
    {
        return Err(invalid(
            "update URL is outside the TrafficMonitor release origin",
        ));
    }
    Ok(())
}
fn parse_update(current: &str, source: u32, xml: &str, language: u16) -> io::Result<UpdateInfo> {
    let fields = xml_fields(xml)?;
    let latest = required(&fields, "root/version")?;
    let section = match source {
        0 => "GitHub",
        1 => "Gitee",
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown update source",
            ));
        }
    };
    for tag in [
        "link",
        "link_x64",
        "link_arm64ec",
        "link_without_temperature",
        "link_without_temperature_x64",
        "link_without_temperature_arm64ec",
    ] {
        if let Some(url) = field(&fields, &format!("root/{section}/{tag}"))? {
            if !url.is_empty() {
                trusted_release_url(url)?;
            }
        }
    }
    let tag = if cfg!(target_arch = "x86") {
        "link"
    } else if cfg!(any(target_arch = "aarch64", target_arch = "arm64ec")) {
        "link_arm64ec"
    } else {
        "link_x64"
    };
    let url = required(&fields, &format!("root/{section}/{tag}"))?;
    trusted_release_url(url)?;
    let notes_tag = match language {
        0x804 | 0x1004 => "contents_zh_cn",
        0x404 | 0xc04 | 0x1404 => "contents_zh_tw",
        _ => "contents_en",
    };
    let notes = field(&fields, &format!("root/update_contents/{notes_tag}"))?
        .or(field(&fields, "root/update_contents/contents_en")?)
        .unwrap_or("")
        .replace("\\n", "\n");
    Ok(UpdateInfo {
        current: current.into(),
        latest: latest.into(),
        available: newer(current, latest)?,
        url: url.into(),
        notes,
    })
}
fn shell_path(path: &str) -> String {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
    }
}
fn quoted_executable(exe: &Path) -> io::Result<String> {
    let s = exe.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "executable path must be Unicode",
        )
    })?;
    if !exe.is_absolute()
        || !exe
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        || s.chars().any(|c| c == '"' || c.is_control())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected an absolute .exe path without quotes or control characters",
        ));
    }
    Ok(format!("\"{}\"", shell_path(s)))
}
fn command_matches(command: &str, exe: &Path) -> io::Result<bool> {
    let quoted = quoted_executable(exe)?;
    let command = command.trim();
    Ok(command.eq_ignore_ascii_case(&quoted)
        || command.eq_ignore_ascii_case(quoted.trim_matches('"')))
}
#[cfg(windows)]
fn can_manage_startup_command(command: &str, exe: &Path) -> io::Result<bool> {
    if command_matches(command, exe)? {
        return Ok(true);
    }
    let command = command.trim();
    let path = if let Some(command) = command.strip_prefix('"') {
        let Some(path) = command.strip_suffix('"') else {
            return Ok(false);
        };
        path
    } else {
        command
    };
    let previous = Path::new(path);
    // Arguments, malformed commands and a matching basename alone do not establish
    // ownership. Version resources cover both the legacy and Rust releases.
    Ok(quoted_executable(previous).is_ok()
        && crate::startup_identity::is_traffic_monitor(previous)
        && crate::startup_identity::is_traffic_monitor(exe))
}
#[derive(Debug, PartialEq, Eq)]
struct TaskInfo {
    command: String,
    enabled: bool,
}
fn task_info(xml: &str) -> io::Result<TaskInfo> {
    let fields = xml_fields(xml)?;
    let command = required(&fields, "Task/Actions/Exec/Command")?;
    if field(&fields, "Task/Actions/Exec/Arguments")?.is_some_and(|s| !s.is_empty()) {
        return Err(invalid(
            "existing TrafficMonitor task contains unexpected arguments",
        ));
    }
    let actions = fields
        .keys()
        .filter(|k| k.starts_with("Task/Actions/") && k.split('/').count() == 3)
        .count();
    if actions != 1 {
        return Err(invalid(
            "existing TrafficMonitor task must have exactly one Exec action",
        ));
    }
    let enabled = match field(&fields, "Task/Settings/Enabled")?.unwrap_or("true") {
        "true" | "1" => true,
        "false" | "0" => false,
        _ => return Err(invalid("invalid task Enabled value")),
    };
    Ok(TaskInfo {
        command: command.into(),
        enabled,
    })
}
fn escape_xml(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}
fn task_xml(exe: &Path, user: &str) -> io::Result<String> {
    let exe = quoted_executable(exe)?;
    let exe = escape_xml(exe.trim_matches('"'));
    let user = escape_xml(user);
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?><Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><RegistrationInfo><Author>{user}</Author><Description>TrafficMonitor user logon startup</Description></RegistrationInfo><Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId><Delay>PT3S</Delay></LogonTrigger></Triggers><Principals><Principal id="User"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals><Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><StartWhenAvailable>false</StartWhenAvailable><Enabled>true</Enabled><ExecutionTimeLimit>PT0S</ExecutionTimeLimit></Settings><Actions Context="User"><Exec><Command>{exe}</Command></Exec></Actions></Task>"#
    ))
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Command, Output, Stdio};
    use std::ptr;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Globalization::{CP_OEMCP, MB_ERR_INVALID_CHARS, MultiByteToWideChar};
    use windows_sys::Win32::Graphics::Dwm::DwmGetColorizationColor;
    use windows_sys::Win32::Networking::WinHttp::*;
    use windows_sys::Win32::System::Registry::*;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const RUN_VALUE: &str = "TrafficMonitor";
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    fn win_error(code: u32) -> io::Error {
        io::Error::from_raw_os_error(code as i32)
    }
    fn success(value: i32) -> io::Result<()> {
        if value != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
    fn reg_string(root: HKEY, key: &str, name: &str) -> io::Result<Option<String>> {
        let key = wide(key);
        let name = wide(name);
        let mut size = 0;
        let result = unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut size,
            )
        };
        if result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND {
            return Ok(None);
        }
        if result != ERROR_SUCCESS {
            return Err(win_error(result));
        }
        if size == 0 || size > 65536 || size % 2 != 0 {
            return Err(invalid("invalid registry string size"));
        }
        let mut data = vec![0u16; (size / 2) as usize];
        let result = unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if result != ERROR_SUCCESS {
            return Err(win_error(result));
        }
        let n = data
            .iter()
            .position(|&c| c == 0)
            .ok_or_else(|| invalid("unterminated registry string"))?;
        if data[n..].iter().any(|&c| c != 0) {
            return Err(invalid("registry command contains embedded null data"));
        }
        Ok(Some(
            String::from_utf16(&data[..n]).map_err(|e| invalid(e.to_string()))?,
        ))
    }
    fn reg_dword(key: &str, name: &str) -> io::Result<Option<u32>> {
        let key = wide(key);
        let name = wide(name);
        let mut value = 0u32;
        let mut size = 4;
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                ptr::null_mut(),
                (&mut value as *mut u32).cast(),
                &mut size,
            )
        };
        match result {
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(None),
            ERROR_SUCCESS if size == 4 => Ok(Some(value)),
            ERROR_SUCCESS => Err(invalid("invalid DWORD registry size")),
            code => Err(win_error(code)),
        }
    }
    pub fn current_theme() -> io::Result<ThemeInfo> {
        let key = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
        let dark = reg_dword(key, "AppsUseLightTheme")?.unwrap_or(1) == 0;
        let system_dark = reg_dword(key, "SystemUsesLightTheme")?.unwrap_or(1) == 0;
        let mut argb = 0;
        let mut opaque = 0;
        let hr = unsafe { DwmGetColorizationColor(&mut argb, &mut opaque) };
        if hr < 0 {
            return Err(io::Error::other(format!(
                "DwmGetColorizationColor failed: 0x{:08x}",
                hr as u32
            )));
        }
        Ok(ThemeInfo {
            dark,
            system_dark,
            color: ((argb >> 16) & 0xff) | (argb & 0xff00) | ((argb & 0xff) << 16),
        })
    }
    pub(super) fn set_run_value_at(
        key: &str,
        name: &str,
        exe: &Path,
        enabled: bool,
    ) -> io::Result<()> {
        let expected = quoted_executable(exe)?;
        let existing = reg_string(HKEY_CURRENT_USER, key, name)?;
        if let Some(current) = &existing {
            if !can_manage_startup_command(current, exe)? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "existing startup value points to another executable; no change made",
                ));
            }
        }
        if !enabled && existing.is_none() {
            return Ok(());
        }
        let path = wide(key);
        let name = wide(name);
        let mut handle = ptr::null_mut();
        let mut result = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                &mut handle,
            )
        };
        if enabled && result == ERROR_FILE_NOT_FOUND {
            result = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, path.as_ptr(), &mut handle) };
        }
        if result != ERROR_SUCCESS {
            return Err(win_error(result));
        }
        let handle = Key(handle);
        // Recheck under the opened key immediately before mutating this one value.
        let current = reg_string(
            handle.0,
            "",
            &String::from_utf16_lossy(&name[..name.len() - 1]),
        )?;
        if current != existing {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "startup command changed during update",
            ));
        }
        let result = if enabled {
            let command = wide(&expected);
            unsafe {
                RegSetValueExW(
                    handle.0,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    command.as_ptr().cast(),
                    (command.len() * 2) as u32,
                )
            }
        } else {
            unsafe { RegDeleteValueW(handle.0, name.as_ptr()) }
        };
        if result == ERROR_SUCCESS || (!enabled && result == ERROR_FILE_NOT_FOUND) {
            Ok(())
        } else {
            Err(win_error(result))
        }
    }
    fn executable(exe: &Path) -> io::Result<PathBuf> {
        quoted_executable(exe)?;
        let path = exe.canonicalize()?;
        if !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "executable must be a file",
            ));
        }
        Ok(path)
    }
    fn system_program(name: &str) -> io::Result<PathBuf> {
        let mut buffer = vec![0u16; 32768];
        let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        if len as usize >= buffer.len() {
            return Err(invalid("system directory path too long"));
        }
        Ok(PathBuf::from(
            String::from_utf16(&buffer[..len as usize]).map_err(|e| invalid(e.to_string()))?,
        )
        .join(name))
    }
    #[link(name = "secur32")]
    unsafe extern "system" {
        fn GetUserNameExW(format: i32, buffer: *mut u16, size: *mut u32) -> u8;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
        fn CoUninitialize();
    }
    fn user_task() -> io::Result<(String, String)> {
        let mut buffer = vec![0u16; 1024];
        let mut size = buffer.len() as u32;
        if unsafe { GetUserNameExW(2, buffer.as_mut_ptr(), &mut size) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let user =
            String::from_utf16(&buffer[..size as usize]).map_err(|e| invalid(e.to_string()))?;
        let name = user
            .rsplit('\\')
            .next()
            .ok_or_else(|| invalid("missing user name"))?;
        if name.is_empty() || name.contains(['\\', '/', '"']) || name.chars().any(char::is_control)
        {
            return Err(invalid("invalid user name for task"));
        }
        Ok((format!("\\TrafficMonitor\\Autorun for {name}"), user))
    }
    fn bounded_output(mut command: Command) -> io::Result<Output> {
        command
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing child stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("missing child stderr"))?;
        let read = |stream: Box<dyn Read + Send>| -> io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            stream
                .take((MAX_RESPONSE + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.len() > MAX_RESPONSE {
                return Err(invalid("helper output exceeds 1 MiB"));
            }
            Ok(bytes)
        };
        let out = std::thread::spawn(move || read(Box::new(stdout)));
        let err = std::thread::spawn(move || read(Box::new(stderr)));
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(error);
                }
            }
            if started.elapsed() > Duration::from_secs(20) {
                let _ = child.kill();
                let _ = child.wait();
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "schtasks exceeded 20 seconds",
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let stdout = out
            .join()
            .map_err(|_| io::Error::other("helper stdout thread failed"))??;
        let stderr = err
            .join()
            .map_err(|_| io::Error::other("helper stderr thread failed"))??;
        Ok(Output {
            status: status?,
            stdout,
            stderr,
        })
    }
    fn schtasks(args: &[String]) -> io::Result<Output> {
        let mut command = Command::new(system_program("schtasks.exe")?);
        command.args(args).arg("/HRESULT");
        bounded_output(command)
    }
    fn decode_output(data: &[u8]) -> io::Result<String> {
        if data.starts_with(&[0xff, 0xfe]) || data.get(1) == Some(&0) {
            let data = data.strip_prefix(&[0xff, 0xfe]).unwrap_or(data);
            if data.len() % 2 != 0 {
                return Err(invalid("odd UTF-16 helper output length"));
            }
            let words = data
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>();
            String::from_utf16(&words).map_err(|e| invalid(e.to_string()))
        } else if let Ok(text) = std::str::from_utf8(data) {
            Ok(text.trim_start_matches('\u{feff}').into())
        } else {
            let size = i32::try_from(data.len()).map_err(|_| invalid("helper output too long"))?;
            let length = unsafe {
                MultiByteToWideChar(
                    CP_OEMCP,
                    MB_ERR_INVALID_CHARS,
                    data.as_ptr(),
                    size,
                    ptr::null_mut(),
                    0,
                )
            };
            if length == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut words = vec![0u16; length as usize];
            let written = unsafe {
                MultiByteToWideChar(
                    CP_OEMCP,
                    MB_ERR_INVALID_CHARS,
                    data.as_ptr(),
                    size,
                    words.as_mut_ptr(),
                    length,
                )
            };
            if written == 0 {
                return Err(io::Error::last_os_error());
            }
            String::from_utf16(&words[..written as usize]).map_err(|e| invalid(e.to_string()))
        }
    }
    fn command_error(output: &Output) -> io::Error {
        let bytes = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        let message = decode_output(bytes)
            .unwrap_or_else(|e| format!("could not decode helper diagnostics: {e}"));
        io::Error::other(format!(
            "schtasks failed (0x{:08x}): {}",
            output.status.code().unwrap_or(-1) as u32,
            message.trim()
        ))
    }
    fn query_task(name: &str) -> io::Result<Option<TaskInfo>> {
        let out = schtasks(&["/Query".into(), "/TN".into(), name.into(), "/XML".into()])?;
        if !out.status.success() {
            return match out.status.code().map(|c| c as u32) {
                Some(0x80070002 | 0x80070003) => Ok(None),
                _ => Err(command_error(&out)),
            };
        }
        task_info(&decode_output(&out.stdout)?).map(Some)
    }
    static TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct TempXml(PathBuf);
    impl Drop for TempXml {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn temporary_xml(contents: &str) -> io::Result<TempXml> {
        let n = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "TrafficMonitor-task-{}-{n}.xml",
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let guard = TempXml(path);
        // Match the declaration and use the BOM-marked encoding accepted by schtasks.
        let bytes: Vec<_> = std::iter::once(0xfeffu16)
            .chain(contents.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        let result = file.write_all(&bytes).and_then(|()| file.sync_all());
        drop(file);
        result?;
        Ok(guard)
    }
    fn set_task(exe: &Path, enabled: bool) -> io::Result<()> {
        let (name, user) = user_task()?;
        let existing = query_task(&name)?;
        if let Some(task) = &existing {
            if !can_manage_startup_command(&task.command, exe)? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "existing TrafficMonitor task points to another executable; no change made",
                ));
            }
        }
        let retarget = existing
            .as_ref()
            .map(|task| command_matches(&task.command, exe).map(|same| !same))
            .transpose()?
            .unwrap_or(false);
        let mut xml = None;
        let args = if !enabled {
            if existing.is_none() {
                return Ok(());
            }
            vec!["/Delete".into(), "/TN".into(), name.clone(), "/F".into()]
        } else if let Some(task) = existing.as_ref().filter(|_| !retarget) {
            if task.enabled {
                return Ok(());
            }
            vec![
                "/Change".into(),
                "/TN".into(),
                name.clone(),
                "/ENABLE".into(),
            ]
        } else {
            xml = Some(temporary_xml(&task_xml(exe, &user)?)?);
            let mut args = vec![
                "/Create".into(),
                "/TN".into(),
                name.clone(),
                "/XML".into(),
                xml.as_ref()
                    .expect("created XML")
                    .0
                    .to_string_lossy()
                    .into_owned(),
            ];
            if retarget {
                args.push("/F".into());
            }
            args
        };
        if retarget && query_task(&name)? != existing {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "startup task changed during update",
            ));
        }
        let out = schtasks(&args)?;
        drop(xml);
        if out.status.success() {
            Ok(())
        } else {
            Err(command_error(&out))
        }
    }
    pub fn auto_start_enabled(exe: &Path, scheduler: bool) -> io::Result<bool> {
        quoted_executable(exe)?;
        if scheduler {
            let (name, _) = user_task()?;
            match query_task(&name)? {
                Some(task) => Ok(task.enabled && command_matches(&task.command, exe)?),
                None => Ok(false),
            }
        } else {
            match reg_string(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE)? {
                Some(command) => command_matches(&command, exe),
                None => Ok(false),
            }
        }
    }
    pub fn set_auto_start(exe: &Path, enabled: bool, scheduler: bool) -> io::Result<()> {
        let exe = executable(exe)?;
        if scheduler {
            set_task(&exe, enabled)?;
            if enabled {
                if let Some(command) = reg_string(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE)? {
                    if can_manage_startup_command(&command, &exe)? {
                        set_run_value_at(RUN_KEY, RUN_VALUE, &exe, false)?;
                    }
                }
            }
        } else {
            set_run_value_at(RUN_KEY, RUN_VALUE, &exe, enabled)?;
            if enabled {
                let (name, _) = user_task()?;
                if let Some(task) = query_task(&name)? {
                    if can_manage_startup_command(&task.command, &exe)? {
                        set_task(&exe, false)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn shell_open(target: &str) -> io::Result<()> {
        if target.is_empty() || target.chars().any(char::is_control) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid shell target",
            ));
        }
        let target = shell_path(target);
        // Shell extensions can require STA. A short-lived dedicated apartment
        // avoids changing the UI/collector thread's existing COM apartment.
        std::thread::spawn(move || {
            let hr = unsafe { CoInitializeEx(ptr::null_mut(), 2 | 4) };
            if hr < 0 {
                return Err(io::Error::other(format!(
                    "shell COM initialization failed: 0x{:08x}",
                    hr as u32
                )));
            }
            struct Apartment;
            impl Drop for Apartment {
                fn drop(&mut self) {
                    unsafe {
                        CoUninitialize();
                    }
                }
            }
            let _apartment = Apartment;
            let operation = wide("open");
            let target = wide(&target);
            let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
            info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC;
            info.lpVerb = operation.as_ptr();
            info.lpFile = target.as_ptr();
            info.nShow = SW_SHOWNORMAL;
            success(unsafe { ShellExecuteExW(&mut info) })
        })
        .join()
        .map_err(|_| io::Error::other("shell worker failed"))?
    }
    pub fn open_path(path: &Path) -> io::Result<()> {
        let path = path.canonicalize()?;
        shell_open(&path.to_string_lossy())
    }
    pub fn open_url(url: &str) -> io::Result<()> {
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "only HTTP(S) URLs may be opened",
                )
            })?;
        if rest.is_empty()
            || rest.starts_with('/')
            || url.len() > 8192
            || url
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid browser URL",
            ));
        }
        shell_open(url)
    }
    pub fn open_task_manager() -> io::Result<()> {
        shell_open(&system_program("Taskmgr.exe")?.to_string_lossy())
    }
    pub fn launch_application(path: &Path) -> io::Result<()> {
        let path = executable(path)?;
        shell_open(&path.to_string_lossy())
    }
    struct Internet(*mut c_void);
    impl Internet {
        fn new(handle: *mut c_void) -> io::Result<Self> {
            if handle.is_null() {
                Err(io::Error::last_os_error())
            } else {
                Ok(Self(handle))
            }
        }
    }
    impl Drop for Internet {
        fn drop(&mut self) {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
    pub(super) fn https_get(url: &str) -> io::Result<String> {
        let (host, path) = https_parts(url)?;
        if !matches!(host, "raw.githubusercontent.com" | "gitee.com") {
            return Err(invalid("untrusted update manifest host"));
        }
        let started = Instant::now();
        let agent = wide("TrafficMonitor-Rust/1.0");
        let session = Internet::new(unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            )
        })?;
        success(unsafe { WinHttpSetTimeouts(session.0, 3000, 5000, 5000, 5000) })?;
        let host = wide(host);
        let connection =
            Internet::new(unsafe { WinHttpConnect(session.0, host.as_ptr(), 443, 0) })?;
        let verb = wide("GET");
        let path = wide(&format!("/{path}"));
        let request = Internet::new(unsafe {
            WinHttpOpenRequest(
                connection.0,
                verb.as_ptr(),
                path.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        })?;
        let redirects = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        success(unsafe {
            WinHttpSetOption(
                request.0,
                WINHTTP_OPTION_REDIRECT_POLICY,
                (&redirects as *const u32).cast(),
                4,
            )
        })?;
        success(unsafe { WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) })?;
        success(unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) })?;
        let mut status = 0u32;
        let mut len = 4;
        success(unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                ptr::null(),
                (&mut status as *mut u32).cast(),
                &mut len,
                ptr::null_mut(),
            )
        })?;
        if status != 200 {
            return Err(io::Error::other(format!(
                "update server returned HTTP {status}"
            )));
        }
        let mut bytes = Vec::new();
        loop {
            if started.elapsed() > Duration::from_secs(20) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "update response exceeded total time budget",
                ));
            }
            let mut block = [0u8; 8192];
            let mut read = 0;
            success(unsafe {
                WinHttpReadData(
                    request.0,
                    block.as_mut_ptr().cast(),
                    block.len() as u32,
                    &mut read,
                )
            })?;
            if read == 0 {
                break;
            }
            if bytes.len() + read as usize > MAX_RESPONSE {
                return Err(invalid("update response exceeds 1 MiB"));
            }
            bytes.extend_from_slice(&block[..read as usize]);
        }
        String::from_utf8(bytes).map_err(|e| invalid(format!("update response is not UTF-8: {e}")))
    }
    pub fn check_update(current: &str, source: u32) -> io::Result<UpdateInfo> {
        version(current)?;
        let url = match source {
            0 => {
                "https://raw.githubusercontent.com/zhongyang219/TrafficMonitor/master/version_utf8.info"
            }
            1 => "https://gitee.com/zhongyang219/TrafficMonitor/raw/master/version_utf8.info",
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unknown update source",
                ));
            }
        };
        let xml = fetch_https(url)?;
        parse_update(current, source, &xml, unsafe { GetUserDefaultUILanguage() })
    }
    #[cfg(test)]
    mod task_scheduler_tests {
        use super::*;

        #[test]
        fn task_scheduler_errors_preserve_native_codepage_text() {
            use std::os::windows::process::ExitStatusExt;
            use windows_sys::Win32::Globalization::{GetOEMCP, WideCharToMultiByte};
            let expected = if unsafe { GetOEMCP() } == 936 {
                "错误: 无法切换编码"
            } else {
                "Error: é"
            };
            let text: Vec<u16> = expected.encode_utf16().collect();
            let mut bytes = vec![0u8; text.len() * 4];
            let length = unsafe {
                WideCharToMultiByte(
                    CP_OEMCP,
                    0,
                    text.as_ptr(),
                    text.len() as i32,
                    bytes.as_mut_ptr(),
                    bytes.len() as i32,
                    ptr::null(),
                    ptr::null_mut(),
                )
            };
            assert!(length > 0);
            bytes.truncate(length as usize);
            let output = Output {
                status: std::process::ExitStatus::from_raw(0x8004131a),
                stdout: Vec::new(),
                stderr: bytes,
            };
            let message = command_error(&output).to_string();
            assert!(message.contains(expected), "{message}");
            assert!(message.contains("0x8004131a"));
        }

        #[test]
        #[ignore = "explicit Task Scheduler import of an owned disabled fixture; never registers a user startup task or runs its action"]
        fn task_scheduler_imports_and_reads_back_generated_xml() {
            let (_, user) = user_task().unwrap();
            let name = format!(
                "\\TrafficMonitorRustTest-Xml-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            assert!(query_task(&name).unwrap().is_none());
            struct OwnedTask {
                name: String,
                created: bool,
            }
            impl Drop for OwnedTask {
                fn drop(&mut self) {
                    if self.created {
                        let _ = schtasks(&[
                            "/Delete".into(),
                            "/TN".into(),
                            self.name.clone(),
                            "/F".into(),
                        ]);
                    }
                }
            }
            let mut task = OwnedTask {
                name,
                created: false,
            };
            // A disabled task/action fixture validates Unicode and XML entities
            // without executing anything or installing a logon startup entry.
            let exe = Path::new(r"C:\Program Files\监控 & 测试\TrafficMonitor.exe");
            let contents = task_xml(exe, &user)
                .unwrap()
                .replace("<Enabled>true</Enabled>", "<Enabled>false</Enabled>");
            let file = temporary_xml(&contents).unwrap();
            let output = schtasks(&[
                "/Create".into(),
                "/TN".into(),
                task.name.clone(),
                "/XML".into(),
                file.0.to_string_lossy().into_owned(),
            ])
            .unwrap();
            assert!(output.status.success(), "{}", command_error(&output));
            task.created = true;
            let info = query_task(&task.name).unwrap().expect("created fixture");
            assert!(!info.enabled);
            assert_eq!(info.command, exe.to_str().unwrap());
            let output = schtasks(&[
                "/Delete".into(),
                "/TN".into(),
                task.name.clone(),
                "/F".into(),
            ])
            .unwrap();
            assert!(output.status.success(), "{}", command_error(&output));
            task.created = false;
            assert!(query_task(&task.name).unwrap().is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_version_comparison_handles_multi_digit_components() {
        assert!(newer("1.9", "1.10").unwrap());
        assert!(!newer("1.10", "1.9").unwrap());
        assert!(!newer("1.2.0", "1.2").unwrap());
    }
    #[test]
    fn stable_versions_sort_above_prereleases() {
        assert!(newer("1.86-beta.9", "1.86-beta.10").unwrap());
        assert!(newer("1.86-beta", "1.86").unwrap());
        assert!(!newer("1.86", "1.86-beta").unwrap());
        assert!(newer("v1.85", "1.86").unwrap());
    }
    #[test]
    fn malformed_versions_are_errors() {
        for value in [
            "",
            "1..2",
            "1.2-",
            "1.2<script>",
            "1.2\n",
            "x1.2",
            "18446744073709551616",
        ] {
            assert!(version(value).is_err(), "{value}");
        }
    }
    #[test]
    fn repository_manifest_parses_both_sources() {
        let xml = include_str!("../../../version_utf8.info");
        for source in 0..=1 {
            let result = parse_update("1.85", source, xml, 0x804).unwrap();
            assert_eq!(result.latest, "1.86");
            assert!(result.available);
            assert!(result.url.starts_with("https://"));
            assert!(result.notes.contains('\n'));
        }
    }
    #[test]
    fn xml_rejects_duplicate_fields_dtd_truncation_and_oversize() {
        for xml in [
            "<root><version>1</version><version>2</version></root>",
            "<!DOCTYPE root><root/>",
            "<root><version>1</root>",
        ] {
            assert!(parse_update("1", 0, xml, 0x409).is_err());
        }
        assert!(xml_fields(&"a".repeat(MAX_RESPONSE + 1)).is_err());
    }
    #[test]
    fn manifest_rejects_untrusted_or_ambiguous_release_urls() {
        for url in [
            "http://github.com/zhongyang219/TrafficMonitor/releases/x",
            "https://github.com.evil.test/zhongyang219/TrafficMonitor/releases/x",
            "https://github.com@evil.test/zhongyang219/TrafficMonitor/releases/x",
            "https://github.com/other/project/releases/x",
            "https://gitee.com\\evil.test/zhongyang219/TrafficMonitor/releases/x",
        ] {
            assert!(trusted_release_url(url).is_err(), "{url}");
        }
    }
    #[test]
    fn task_xml_roundtrips_entities_and_exact_command() {
        if cfg!(windows) {
            let path = Path::new("C:\\Program Files\\Traffic & Monitor\\TrafficMonitor.exe");
            let xml = task_xml(path, "DOMAIN\\User & Name").unwrap();
            let task = task_info(&xml).unwrap();
            assert!(task.enabled);
            assert_eq!(task.command, path.to_str().unwrap());
            assert!(command_matches(&task.command, path).unwrap());
            assert!(xml.contains("PT3S"));
            assert!(xml.contains("InteractiveToken"));
        }
    }
    #[test]
    fn task_ownership_rejects_arguments_and_multiple_actions() {
        for xml in [
            "<Task><Actions><Exec><Command>C:\\TrafficMonitor.exe</Command><Arguments>--other</Arguments></Exec></Actions></Task>",
            "<Task><Actions><Exec><Command>C:\\TrafficMonitor.exe</Command></Exec><Exec><Command>C:\\other.exe</Command></Exec></Actions></Task>",
        ] {
            assert!(task_info(xml).is_err());
        }
    }
    #[cfg(windows)]
    #[test]
    fn registry_quotes_and_ownership_are_exact() {
        let exe = Path::new("C:\\Program Files\\TrafficMonitor.exe");
        assert_eq!(
            quoted_executable(exe).unwrap(),
            "\"C:\\Program Files\\TrafficMonitor.exe\""
        );
        assert!(command_matches("\"c:\\program files\\trafficmonitor.exe\"", exe).unwrap());
        assert!(
            !command_matches("\"C:\\Program Files\\TrafficMonitor.exe\" --other", exe).unwrap()
        );
        assert!(!command_matches("C:\\Other.exe", exe).unwrap());
        assert!(quoted_executable(Path::new("relative.exe")).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn invalid_inputs_never_launch_or_contact_a_server() {
        assert!(open_url("javascript:alert(1)").is_err());
        assert!(open_url("https://example.test/\n").is_err());
        assert!(launch_application(Path::new("missing.exe")).is_err());
        assert!(open_path(Path::new("missing-trafficmonitor-test-path")).is_err());
        assert!(check_update("1.0", 999).is_err());
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    #[test]
    fn xml_empty_elements_obey_node_limit() {
        let xml = format!("<root>{}</root>", "<a/>".repeat(4096));
        assert!(xml_fields(&xml).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn isolated_registry_fixture_preserves_foreign_commands() {
        use std::ptr;
        use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows_sys::Win32::System::Registry::*;
        let path = format!(
            "Software\\TrafficMonitorRustTests\\Services-{}",
            std::process::id()
        );
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        struct Cleanup(Vec<u16>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                unsafe {
                    RegDeleteKeyW(HKEY_CURRENT_USER, self.0.as_ptr());
                }
            }
        }
        let _cleanup = Cleanup(wide(&path));
        let exe = Path::new("C:\\Program Files\\TrafficMonitor.exe");
        let other = Path::new("C:\\Other\\TrafficMonitor.exe");
        platform::set_run_value_at(&path, "Fixture", exe, true).unwrap();
        assert_eq!(
            platform::set_run_value_at(&path, "Fixture", other, true)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            platform::set_run_value_at(&path, "Fixture", other, false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        let mut buffer = [0u16; 256];
        let mut size = 512;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(&path).as_ptr(),
                wide("Fixture").as_ptr(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        assert_eq!(status, 0);
        let text = String::from_utf16(&buffer[..(size as usize / 2) - 1]).unwrap();
        assert_eq!(text, "\"C:\\Program Files\\TrafficMonitor.exe\"");
        platform::set_run_value_at(&path, "Fixture", exe, false).unwrap();
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(&path).as_ptr(),
                wide("Fixture").as_ptr(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut size,
            )
        };
        assert_eq!(status, ERROR_FILE_NOT_FOUND);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires TM_STARTUP_FIXTURE_EXE pointing to a built TrafficMonitor; writes only an isolated test registry key"]
    fn isolated_registry_fixture_migrates_a_relocated_traffic_monitor() {
        use std::io::Write;
        use windows_sys::Win32::System::Registry::*;
        let fixture = std::env::var_os("TM_STARTUP_FIXTURE_EXE")
            .expect("set TM_STARTUP_FIXTURE_EXE to a real TrafficMonitor build");
        let root = std::env::temp_dir().join(format!(
            "TrafficMonitor-startup-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let key = format!(
            "Software\\TrafficMonitorRustTests\\StartupMigration-{}",
            std::process::id()
        );
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        struct Cleanup {
            root: std::path::PathBuf,
            key: Vec<u16>,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, self.key.as_ptr()) };
                for file in [
                    "Old install/TrafficMonitor.exe",
                    "New install/TrafficMonitor-Updated.exe",
                    "Other/TrafficMonitor.exe",
                ] {
                    let _ = std::fs::remove_file(self.root.join(file));
                }
                for directory in ["Old install", "New install", "Other"] {
                    let _ = std::fs::remove_dir(self.root.join(directory));
                }
                let _ = std::fs::remove_dir(&self.root);
            }
        }
        let _cleanup = Cleanup {
            root: root.clone(),
            key: wide(&key),
        };
        let old = root.join("Old install/TrafficMonitor.exe");
        let new = root.join("New install/TrafficMonitor-Updated.exe");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::copy(&fixture, &old).unwrap();
        std::fs::copy(&fixture, &new).unwrap();
        // A valid PE overlay keeps version resources but makes the two builds differ.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&new)
            .unwrap()
            .write_all(b"\nstartup migration fixture\n")
            .unwrap();
        let command = || {
            let mut data = [0u16; 2048];
            let mut size = std::mem::size_of_val(&data) as u32;
            let status = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    wide(&key).as_ptr(),
                    wide("Fixture").as_ptr(),
                    RRF_RT_REG_SZ,
                    std::ptr::null_mut(),
                    data.as_mut_ptr().cast(),
                    &mut size,
                )
            };
            if status == windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND {
                return None;
            }
            assert_eq!(status, 0);
            Some(String::from_utf16(&data[..size as usize / 2 - 1]).unwrap())
        };
        platform::set_run_value_at(&key, "Fixture", &old, true).unwrap();
        platform::set_run_value_at(&key, "Fixture", &new, true)
            .expect("an existing TrafficMonitor entry must follow its new install path");
        assert_eq!(command(), Some(quoted_executable(&new).unwrap()));

        let foreign = root.join("Other/TrafficMonitor.exe");
        std::fs::create_dir_all(foreign.parent().unwrap()).unwrap();
        std::fs::copy(
            std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join("System32/cmd.exe"),
            &foreign,
        )
        .unwrap();
        // Same filename is insufficient: preserve a different product's command.
        platform::set_run_value_at(&key, "Fixture", &new, false).unwrap();
        platform::set_run_value_at(&key, "Fixture", &foreign, true).unwrap();
        assert!(platform::set_run_value_at(&key, "Fixture", &new, true).is_err());
        assert!(platform::set_run_value_at(&key, "Fixture", &new, false).is_err());
        assert_eq!(command(), Some(quoted_executable(&foreign).unwrap()));
        platform::set_run_value_at(&key, "Fixture", &foreign, false).unwrap();
        platform::set_run_value_at(&key, "Fixture", &old, true).unwrap();
        platform::set_run_value_at(&key, "Fixture", &new, false).unwrap();
        assert_eq!(command(), None);
    }
}

#[cfg(all(test, windows))]
mod path_tests {
    use super::*;
    #[test]
    fn extended_unc_path_keeps_network_authority() {
        let path = Path::new(r"\\?\UNC\server\share\TrafficMonitor.exe");
        assert_eq!(
            quoted_executable(path).unwrap(),
            r#""\\server\share\TrafficMonitor.exe""#
        );
    }
}
