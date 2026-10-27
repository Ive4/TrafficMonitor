//! Strict parsing and read-only checks of the official plugin version manifest.
//!
//! No URLs are accepted from the XML, no plugin bytes are downloaded or executed,
//! and a malformed manifest never yields a partially usable version catalogue.
use quick_xml::{
    Reader,
    encoding::Decoder,
    events::{BytesStart, Event},
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fmt, io,
};
const MAX_MANIFEST: usize = 1024 * 1024;
const MAX_PLUGINS: usize = 4096;

#[derive(Clone, Debug)]
pub struct PluginVersion {
    pub text: String,
    components: Vec<u32>,
}
impl PluginVersion {
    pub fn parse(value: &str) -> Result<Self, String> {
        let text = value.trim();
        let numeric = text.strip_prefix(['v', 'V']).unwrap_or(text);
        if numeric.is_empty() || text.len() > 256 {
            return Err("Missing or oversized plugin version".into());
        }
        let parts: Vec<_> = numeric.split('.').collect();
        if parts.len() > 32 {
            return Err("Too many plugin version components".into());
        }
        let mut components = Vec::new();
        for part in parts {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(format!("Invalid numeric plugin version: {text}"));
            }
            components.push(
                part.parse::<u32>()
                    .map_err(|_| format!("Plugin version component overflows u32: {part}"))?,
            );
        }
        while components.len() > 1 && components.last() == Some(&0) {
            components.pop();
        }
        Ok(Self {
            text: text.into(),
            components,
        })
    }
}
impl PartialEq for PluginVersion {
    fn eq(&self, other: &Self) -> bool {
        self.components == other.components
    }
}
impl Eq for PluginVersion {}
impl PartialOrd for PluginVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for PluginVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.components.cmp(&other.components)
    }
}
impl fmt::Display for PluginVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginUpdate {
    pub file_name: String,
    pub current: String,
    pub latest: String,
}
#[derive(Clone, Debug, Default)]
pub struct UpdateManifest {
    /// Case-insensitive Windows filenames, normalized to lowercase.
    pub entries: BTreeMap<String, PluginVersion>,
}
impl UpdateManifest {
    pub fn parse(xml: &str) -> Result<Self, String> {
        if xml.len() > MAX_MANIFEST {
            return Err("Plugin manifest exceeds 1 MiB".into());
        }
        let mut reader = Reader::from_str(xml);
        let mut stack: Vec<String> = Vec::new();
        let mut started = false;
        let mut result = Self::default();
        loop {
            let event = reader
                .read_event()
                .map_err(|e| format!("Malformed plugin XML: {e}"))?;
            let empty = matches!(event, Event::Empty(_));
            match event {
                Event::Start(start) | Event::Empty(start) => {
                    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
                    if stack.is_empty() {
                        if started || name != "plugins" {
                            return Err(
                                "Plugin manifest requires exactly one <plugins> root".into()
                            );
                        }
                        if start.attributes().next().is_some() {
                            return Err("Unexpected attributes on plugin manifest root".into());
                        }
                        started = true;
                    } else if stack.len() == 1 && stack[0] == "plugins" && name == "plugin" {
                        let (name, version) = entry(&start, reader.decoder())?;
                        if result.entries.len() >= MAX_PLUGINS {
                            return Err("Plugin manifest exceeds entry limit".into());
                        }
                        let normalized = name.to_lowercase();
                        if result.entries.insert(normalized.clone(), version).is_some() {
                            return Err(format!("Duplicate plugin filename: {normalized}"));
                        }
                    } else {
                        return Err("Unexpected nested element in plugin manifest".into());
                    }
                    if !empty {
                        stack.push(name);
                    }
                }
                Event::End(end) => {
                    let name = stack.pop().ok_or("Unexpected closing plugin XML tag")?;
                    if end.name().as_ref() != name.as_bytes() {
                        return Err("Mismatched closing plugin XML tag".into());
                    }
                }
                Event::Text(text) => {
                    if !text.decode().map_err(|e| e.to_string())?.trim().is_empty() {
                        return Err("Plugin manifest contains unexpected text".into());
                    }
                }
                Event::Comment(_) => {}
                Event::Decl(decl) => {
                    if started {
                        return Err("XML declaration is not at the beginning".into());
                    }
                    if let Some(encoding) = decl.encoding() {
                        let encoding = encoding.map_err(|e| e.to_string())?;
                        if !encoding.eq_ignore_ascii_case(b"utf-8")
                            && !encoding.eq_ignore_ascii_case(b"utf8")
                        {
                            return Err("Plugin manifest must be UTF-8".into());
                        }
                    }
                }
                Event::Eof => break,
                Event::DocType(_) => {
                    return Err("Plugin manifest DTD/entity declarations are prohibited".into());
                }
                _ => {
                    return Err(
                        "Unexpected processing instruction, entity or CDATA in plugin manifest"
                            .into(),
                    );
                }
            }
        }
        if !started || !stack.is_empty() {
            return Err("Empty or unclosed plugin manifest".into());
        }
        Ok(result)
    }
    pub fn latest(&self, file_name: &str) -> Option<&PluginVersion> {
        self.entries.get(&file_name.to_lowercase())
    }
    pub fn updates(&self, installed: &[(String, String)]) -> Result<Vec<PluginUpdate>, String> {
        let mut updates = Vec::new();
        let mut seen = BTreeSet::new();
        for (file, current) in installed {
            validate_filename(file)?;
            if !seen.insert(file.to_lowercase()) {
                return Err(format!("Duplicate installed plugin filename: {file}"));
            }
            let Some(latest) = self.latest(file) else {
                continue;
            };
            let version = PluginVersion::parse(current)?;
            if latest > &version {
                updates.push(PluginUpdate {
                    file_name: file.clone(),
                    current: current.clone(),
                    latest: latest.text.clone(),
                });
            }
        }
        Ok(updates)
    }
}
fn entry(start: &BytesStart<'_>, decoder: Decoder) -> Result<(String, PluginVersion), String> {
    let mut name = None;
    let mut version = None;
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| e.to_string())?;
        let value = attribute
            .decode_and_unescape_value(decoder)
            .map_err(|e| e.to_string())?
            .into_owned();
        match attribute.key.as_ref() {
            b"file_name" => name = Some(value),
            b"version" => version = Some(value),
            _ => return Err("Unknown attribute in plugin version entry".into()),
        }
    }
    let name = name.ok_or("Plugin entry is missing file_name")?;
    validate_filename(&name)?;
    let version = PluginVersion::parse(&version.ok_or("Plugin entry is missing version")?)?;
    Ok((name, version))
}
fn validate_filename(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 1024
        || name.chars().count() > 255
        || name.trim() != name
        || name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
        || !name.to_ascii_lowercase().ends_with(".dll")
        || name.len() <= 4
    {
        return Err(format!("Invalid plugin DLL filename: {name:?}"));
    }
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || base
            .strip_prefix("COM")
            .or_else(|| base.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'));
    if reserved {
        return Err(format!("Reserved Windows plugin filename: {name}"));
    }
    Ok(())
}
pub fn manifest_url(source: u32) -> io::Result<&'static str> {
    match source {
        0 => Ok(
            "https://raw.githubusercontent.com/zhongyang219/TrafficMonitorPlugins/main/plugins_version.xml",
        ),
        1 => {
            Ok("https://gitee.com/zhongyang219/TrafficMonitorPlugins/raw/main/plugins_version.xml")
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unknown plugin update source",
        )),
    }
}
pub fn fetch_manifest(source: u32) -> io::Result<UpdateManifest> {
    let xml = crate::services::fetch_https(manifest_url(source)?)?;
    UpdateManifest::parse(&xml).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
pub fn check_updates(source: u32, installed: &[(String, String)]) -> io::Result<Vec<PluginUpdate>> {
    manifest_url(source)?;
    if installed.is_empty() {
        return Ok(Vec::new());
    }
    fetch_manifest(source)?
        .updates(installed)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
