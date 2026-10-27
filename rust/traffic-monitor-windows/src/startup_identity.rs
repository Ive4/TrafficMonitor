//! Identify a previous TrafficMonitor installation without executing its binary.
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};

fn matches_product(product: &str, original: &str, company: &str) -> bool {
    product.eq_ignore_ascii_case("TrafficMonitor")
        && original.eq_ignore_ascii_case("TrafficMonitor.exe")
        && (company.eq_ignore_ascii_case("zhongyang219")
            || company.eq_ignore_ascii_case("By ZhongYang"))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn query<'a>(data: &'a [u32], field: &str, string: bool) -> Option<&'a [u8]> {
    let mut value = ptr::null_mut();
    let mut length = 0;
    if unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            wide(field).as_ptr(),
            &mut value,
            &mut length,
        )
    } == 0
        || value.is_null()
    {
        return None;
    }
    let bytes = (length as usize).checked_mul(if string { 2 } else { 1 })?;
    let start = value as usize;
    let end = start.checked_add(bytes)?;
    let buffer_start = data.as_ptr() as usize;
    if start < buffer_start || end > buffer_start + std::mem::size_of_val(data) {
        return None;
    }
    // The checked range belongs to the live, initialized version-info buffer.
    Some(unsafe { std::slice::from_raw_parts(value.cast(), bytes) })
}

fn string(data: &[u32], field: &str) -> Option<String> {
    let bytes = query(data, field, true)?;
    let words: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    String::from_utf16(words.strip_suffix(&[0])?).ok()
}

pub(crate) fn is_traffic_monitor(path: &Path) -> bool {
    let Some(path) = path.to_str() else {
        return false;
    };
    let path = wide(path);
    let mut handle = 0;
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut handle) };
    if size == 0 || size > 1024 * 1024 {
        return false;
    }
    // DWORD alignment also covers every language/codepage and UTF-16 field.
    let mut data = vec![0u32; (size as usize).div_ceil(4)];
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return false;
    }
    let Some(translations) = query(&data, r"\VarFileInfo\Translation", false) else {
        return false;
    };
    translations.chunks_exact(4).any(|t| {
        let language = u16::from_le_bytes([t[0], t[1]]);
        let codepage = u16::from_le_bytes([t[2], t[3]]);
        let field = |name| {
            string(
                &data,
                &format!(r"\StringFileInfo\{language:04x}{codepage:04x}\{name}"),
            )
            .unwrap_or_default()
        };
        matches_product(
            &field("ProductName"),
            &field("OriginalFilename"),
            &field("CompanyName"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_legacy_and_rust_product_resources_but_not_a_filename_alone() {
        assert!(matches_product(
            "TrafficMonitor",
            "TrafficMonitor.exe",
            "zhongyang219"
        ));
        assert!(matches_product(
            "TrafficMonitor",
            "TrafficMonitor.exe",
            "By ZhongYang"
        ));
        assert!(!matches_product(
            "Other product",
            "TrafficMonitor.exe",
            "zhongyang219"
        ));
        assert!(!matches_product(
            "TrafficMonitor",
            "Other.exe",
            "zhongyang219"
        ));
        assert!(!matches_product("TrafficMonitor", "TrafficMonitor.exe", ""));
        assert!(!is_traffic_monitor(Path::new("missing-TrafficMonitor.exe")));
    }
}
