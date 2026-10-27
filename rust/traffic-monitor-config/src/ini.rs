use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Debug)]
struct Line {
    raw: String,
    section: String,
    key: Option<String>,
    value: String,
}
/// Ordered, lossless for untouched lines. The first duplicate wins, matching CIniHelper.
#[derive(Clone, Debug, Default)]
pub struct IniDocument {
    lines: Vec<Line>,
}
impl IniDocument {
    pub fn parse(text: &str) -> Self {
        let mut section = String::new();
        let mut lines = Vec::new();
        for raw in text.trim_start_matches('\u{feff}').lines() {
            let trimmed = raw.trim();
            let mut key = None;
            let mut value = String::new();
            if let Some(s) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                section = s.trim().to_owned();
            } else if !trimmed.starts_with([';', '#']) {
                if let Some((k, v)) = trimmed.split_once('=') {
                    if !k.trim().is_empty() {
                        key = Some(k.trim().into());
                        value = decode_value(v.trim());
                    }
                }
            }
            lines.push(Line {
                raw: raw.into(),
                section: section.clone(),
                key,
                value,
            });
        }
        Self { lines }
    }
    pub fn parse_bytes(bytes: &[u8]) -> io::Result<Self> {
        Ok(Self::parse(&decode_bytes(bytes)?))
    }
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::parse_bytes(&fs::read(path)?)
    }
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.lines
            .iter()
            .find(|l| l.section == section && l.key.as_deref() == Some(key))
            .map(|l| l.value.as_str())
    }
    pub fn get_string(&self, section: &str, key: &str, default: &str) -> String {
        self.get(section, key).unwrap_or(default).into()
    }
    pub fn get_bool(&self, section: &str, key: &str, default: bool) -> bool {
        self.get(section, key)
            .map(|v| match v {
                "true" => true,
                "false" => false,
                _ => legacy_i64(v) != 0,
            })
            .unwrap_or(default)
    }
    pub fn get_i64(&self, section: &str, key: &str, default: i64) -> i64 {
        self.get(section, key).map(legacy_i64).unwrap_or(default)
    }
    pub fn set(&mut self, section: &str, key: &str, value: impl Into<String>) {
        let value = value.into();
        let raw = format!("{key} = {}", encode_value(&value));
        if let Some(line) = self
            .lines
            .iter_mut()
            .find(|l| l.section == section && l.key.as_deref() == Some(key))
        {
            line.value = value;
            line.raw = raw;
            return;
        }
        let start = self.lines.iter().position(|l| l.section == section);
        let at = if let Some(start) = start {
            self.lines
                .iter()
                .enumerate()
                .skip(start + 1)
                .find(|(_, l)| l.section != section)
                .map(|(i, _)| i)
                .unwrap_or(self.lines.len())
        } else {
            if !section.is_empty() {
                self.lines.push(Line {
                    raw: format!("[{section}]"),
                    section: section.into(),
                    key: None,
                    value: String::new(),
                });
            }
            self.lines.len()
        };
        self.lines.insert(
            at,
            Line {
                raw,
                section: section.into(),
                key: Some(key.into()),
                value,
            },
        );
    }
    pub fn set_bool(&mut self, s: &str, k: &str, v: bool) {
        self.set(s, k, if v { "true" } else { "false" })
    }
    pub fn set_i64(&mut self, s: &str, k: &str, v: i64) {
        self.set(s, k, v.to_string())
    }
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.lines.iter().filter_map(|l| {
            l.key
                .as_deref()
                .map(|k| (l.section.as_str(), k, l.value.as_str()))
        })
    }
    pub fn sections(&self) -> Vec<&str> {
        let mut result = Vec::new();
        for l in &self.lines {
            if !l.section.is_empty() && !result.contains(&l.section.as_str()) {
                result.push(l.section.as_str());
            }
        }
        result
    }
    pub fn remove_section(&mut self, section: &str) {
        self.lines.retain(|l| l.section != section)
    }
    pub fn get_i64_array(&self, s: &str, k: &str, len: usize, default: i64) -> Vec<i64> {
        let mut a: Vec<_> = self
            .get(s, k)
            .unwrap_or("")
            .split(',')
            .filter(|v| !v.trim().is_empty())
            .map(legacy_i64)
            .take(len)
            .collect();
        let last = a.last().copied().unwrap_or(default);
        a.resize(len, last);
        a
    }
    pub fn set_i64_array(&mut self, s: &str, k: &str, a: &[i64]) {
        self.set(
            s,
            k,
            a.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        )
    }
    pub fn get_bool_array(&self, s: &str, k: &str, len: usize) -> Vec<bool> {
        let bits = self.get_i64(s, k, 0) as u64;
        (0..len)
            .map(|i| i < 64 && (bits & (1u64 << i)) != 0)
            .collect()
    }
    pub fn set_bool_array(&mut self, s: &str, k: &str, a: &[bool]) {
        let bits = a
            .iter()
            .take(64)
            .enumerate()
            .fold(0i64, |n, (i, b)| n | ((*b as i64) << i));
        self.set_i64(s, k, bits)
    }
    pub fn get_string_list(&self, s: &str, k: &str) -> Vec<String> {
        // Use raw representation because quotes delimit legacy comma lists.
        let Some(raw) = self
            .lines
            .iter()
            .find(|l| l.section == s && l.key.as_deref() == Some(k))
            .and_then(|l| l.raw.split_once('=').map(|(_, v)| v.trim()))
        else {
            return Vec::new();
        };
        if raw.is_empty() {
            return Vec::new();
        }
        let mut values = Vec::new();
        let mut current = String::new();
        let mut quoted = false;
        let mut escape = false;
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if escape {
                current.push('\\');
                current.push(c);
                escape = false;
            } else if c == '\\' && !is_windows_path(&current) {
                escape = true;
            } else if c == '"' {
                if quoted && chars.peek() == Some(&'"') {
                    current.push_str("\\\"");
                    chars.next();
                } else {
                    quoted = !quoted;
                }
            } else if c == ',' && !quoted {
                values.push(decode_list_item(&current));
                current.clear();
            } else {
                current.push(c);
            }
        }
        if escape {
            current.push('\\');
        }
        values.push(decode_list_item(&current));
        values
    }
    pub fn set_string_list(&mut self, s: &str, k: &str, values: &[String]) {
        self.set(s, k, "");
        let raw = values
            .iter()
            .map(|v| encode_value(v))
            .collect::<Vec<_>>()
            .join(",");
        if let Some(l) = self
            .lines
            .iter_mut()
            .find(|l| l.section == s && l.key.as_deref() == Some(k))
        {
            l.raw = format!("{k} = {raw}");
            l.value = decode_value(&raw);
        }
    }
    pub fn to_ini_string(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            out.push_str(&l.raw);
            out.push_str("\r\n");
        }
        out
    }
    /// Write a sibling file, flush it, then atomically replace. Existing files survive failures.
    pub fn save_atomic(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let mut bytes = b"\xef\xbb\xbf".to_vec();
        bytes.extend_from_slice(self.to_ini_string().as_bytes());
        save_bytes_atomic(path, &bytes)
    }
}
/// Atomically persist arbitrary bytes. No backups, directory creation or data transformation.
pub fn save_bytes_atomic(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let path = path.as_ref();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "configuration path has no filename",
            )
        })?
        .to_string_lossy();
    let mut created = None;
    for _ in 0..32 {
        let temp = parent.join(format!(
            ".{name}.{}.{}.tmp",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(f) => {
                created = Some((temp, f));
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    let (temp, mut file) = created.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot reserve configuration temporary file",
        )
    })?;
    let result = (|| {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        replace(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub(crate) fn legacy_i64(v: &str) -> i64 {
    let v = v.trim_start();
    let n = v
        .char_indices()
        .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && matches!(c, '-' | '+')))
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    v[..n].parse().unwrap_or(0)
}
fn decode_value(v: &str) -> String {
    let v = if v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('$') && v.ends_with('$')))
    {
        &v[1..v.len() - 1]
    } else {
        v
    };
    if is_windows_path(v) {
        v.replace("\"\"", "\"")
    } else {
        unescape(v)
    }
}
// Old WriteString writes Windows paths verbatim. A path's \t and \n are not escapes.
fn is_windows_path(v: &str) -> bool {
    v.as_bytes().get(1) == Some(&b':') && v.as_bytes().get(2) == Some(&b'\\')
        || v.starts_with(".\\")
        || v.starts_with("..\\")
        || v.starts_with("\\\\")
}
fn decode_list_item(v: &str) -> String {
    if is_windows_path(v) {
        v.replace("\\\"", "\"")
    } else {
        unescape(v)
    }
}
pub fn unescape(v: &str) -> String {
    let mut out = String::new();
    let mut chars = v.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(c @ ('"' | ';' | '#' | '\\')) => out.push(c),
                Some(c) => {
                    out.push('\\');
                    out.push(c);
                }
                None => out.push('\\'),
            }
        } else if c == '"' && chars.peek() == Some(&'"') {
            out.push(c);
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}
fn escape(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}
fn encode_value(v: &str) -> String {
    if is_windows_path(v) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        format!("\"{}\"", escape(v))
    }
}
pub fn decode_bytes(bytes: &[u8]) -> io::Result<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated UTF-16",
            ));
        }
        let le = bytes[0] == 0xff;
        let units: Vec<_> = bytes[2..]
            .chunks_exact(2)
            .map(|p| {
                if le {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            })
            .collect();
        return String::from_utf16(&units)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
    }
    if let Some(bytes) = bytes.strip_prefix(b"\xef\xbb\xbf") {
        return String::from_utf8(bytes.to_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
    }
    match std::str::from_utf8(bytes) {
        Ok(v) => Ok(v.into()),
        Err(_) => decode_ansi(bytes),
    }
}
#[cfg(windows)]
fn decode_ansi(bytes: &[u8]) -> io::Result<String> {
    use windows_sys::Win32::Globalization::{CP_ACP, MultiByteToWideChar};
    if bytes.is_empty() {
        return Ok(String::new());
    }
    let len = i32::try_from(bytes.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "INI exceeds Windows codepage limit",
        )
    })?;
    unsafe {
        let needed = MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), len, std::ptr::null_mut(), 0);
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut wide = vec![0u16; needed as usize];
        if MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), len, wide.as_mut_ptr(), needed) == 0 {
            return Err(io::Error::last_os_error());
        }
        String::from_utf16(&wide).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}
#[cfg(not(windows))]
fn decode_ansi(_bytes: &[u8]) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "legacy ANSI requires Windows ACP",
    ))
}
#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let a: Vec<_> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<_> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            a.as_ptr(),
            b.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}
