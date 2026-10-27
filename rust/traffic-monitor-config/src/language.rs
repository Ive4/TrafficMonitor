use crate::IniDocument;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Default)]
pub struct LanguageInfo {
    pub bcp_47: String,
    pub display_name: String,
    pub default_font: String,
    pub translator: String,
    pub translator_url: String,
    pub path: PathBuf,
}
impl LanguageInfo {
    pub fn config_string(&self) -> String {
        format!("{}|{}|{}", self.bcp_47, self.display_name, self.translator)
    }
    pub fn language_id(&self) -> u32 {
        locale_id(&self.bcp_47)
    }
}
#[derive(Clone, Debug)]
pub struct Language {
    pub info: LanguageInfo,
    document: IniDocument,
    fallback: IniDocument,
}
impl Language {
    /// Host plugin cache keys follow its ABI: (string key, section).
    pub fn strings(&self) -> BTreeMap<(String, String), String> {
        let mut strings = BTreeMap::new();
        for document in [&self.fallback, &self.document] {
            // IniDocument::get uses the first duplicate, so keep that same rule per pack.
            let mut pack = BTreeMap::new();
            for (section, key, value) in document.entries() {
                pack.entry((key.to_owned(), section.to_owned()))
                    .or_insert_with(|| value.to_owned());
            }
            strings.extend(pack);
        }
        strings
    }
    pub fn text(&self, section: &str, key: &str, fallback: &str) -> String {
        self.document
            .get(section, key)
            .or_else(|| self.fallback.get(section, key))
            .unwrap_or(fallback)
            .into()
    }
    pub fn format(&self, section: &str, key: &str, args: &[&str], fallback: &str) -> String {
        format_parameters(&self.text(section, key, fallback), args)
    }
}
#[derive(Clone, Debug)]
pub struct LanguageCatalog {
    pub languages: Vec<LanguageInfo>,
    documents: BTreeMap<String, IniDocument>,
    fallback: IniDocument,
}
impl LanguageCatalog {
    /// The English resource is compiled into Rust so missing external packs remain usable.
    pub fn load(dir: impl AsRef<Path>) -> io::Result<Self> {
        let fallback =
            IniDocument::parse(include_str!("../../../TrafficMonitor/language/English.ini"));
        let mut catalog = Self {
            languages: Vec::new(),
            documents: BTreeMap::new(),
            fallback,
        };
        for text in [
            include_str!("../../../TrafficMonitor/language/English.ini"),
            include_str!("../../../TrafficMonitor/language/Simplified_Chinese.ini"),
            include_str!("../../../TrafficMonitor/language/Traditional_Chinese.ini"),
        ] {
            catalog.insert(IniDocument::parse(text), PathBuf::new())?;
        }
        match fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry?.path();
                    if path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("ini"))
                    {
                        catalog.insert(IniDocument::load(&path)?, path)?;
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        catalog.languages.sort_by(|a, b| a.bcp_47.cmp(&b.bcp_47));
        Ok(catalog)
    }
    fn insert(&mut self, ini: IniDocument, path: PathBuf) -> io::Result<()> {
        let tag = ini.get_string("general", "BCP_47", "");
        if tag.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("language {} has no BCP_47", path.display()),
            ));
        }
        let info = LanguageInfo {
            bcp_47: tag.clone(),
            display_name: ini.get_string("general", "DISPLAY_NAME", &tag),
            default_font: ini.get_string("general", "DEFAULT_FONT", "Microsoft Sans Serif"),
            translator: ini.get_string("general", "TRANSLATOR", "<Unknown>"),
            translator_url: ini.get_string("general", "TRANSLATOR_URL", ""),
            path,
        };
        let identity = info.config_string();
        self.languages.retain(|i| i.config_string() != identity);
        self.languages.push(info);
        self.documents.insert(identity, ini);
        Ok(())
    }
    pub fn select(&self, requested: &str) -> Language {
        let requested_tag = requested.split('|').next().unwrap_or("");
        let system = if requested_tag.is_empty() || requested_tag == "0" {
            system_language()
        } else {
            requested_tag.into()
        };
        let tag = system.replace('_', "-").to_ascii_lowercase();
        let matched = self
            .languages
            .iter()
            .find(|l| l.config_string() == requested)
            .or_else(|| {
                let name = requested.split('|').nth(1)?;
                self.languages
                    .iter()
                    .find(|l| l.bcp_47.eq_ignore_ascii_case(&tag) && l.display_name == name)
            })
            .or_else(|| {
                self.languages
                    .iter()
                    .find(|l| l.bcp_47.eq_ignore_ascii_case(&tag))
            })
            .or_else(|| {
                self.languages.iter().find(|l| {
                    l.bcp_47
                        .split('-')
                        .next()
                        .unwrap_or("")
                        .eq_ignore_ascii_case(tag.split('-').next().unwrap_or(""))
                })
            })
            .or_else(|| {
                self.languages
                    .iter()
                    .find(|l| l.bcp_47.eq_ignore_ascii_case("en-US"))
            });
        let info = matched.cloned().unwrap_or_default();
        let document = self
            .documents
            .get(&info.config_string())
            .cloned()
            .unwrap_or_else(|| self.fallback.clone());
        Language {
            info,
            document,
            fallback: self.fallback.clone(),
        }
    }
}
/// One-pass replacement avoids interpreting placeholders introduced by arguments.
pub fn format_parameters(template: &str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("<%") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        if let Some(end) = tail.find("%>") {
            let index = tail[..end]
                .parse::<usize>()
                .ok()
                .and_then(|i| i.checked_sub(1));
            if let Some(arg) = index.and_then(|i| args.get(i)) {
                out.push_str(arg);
            } else {
                out.push_str(&rest[start..start + 2 + end + 2]);
            }
            rest = &tail[end + 2..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}
#[cfg(windows)]
pub fn system_language() -> String {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if len > 1 {
        String::from_utf16_lossy(&buf[..len as usize - 1])
    } else {
        "en-US".into()
    }
}
#[cfg(not(windows))]
pub fn system_language() -> String {
    std::env::var("LANG")
        .unwrap_or_else(|_| "en-US".into())
        .split('.')
        .next()
        .unwrap_or("en-US")
        .replace('_', "-")
}
#[cfg(windows)]
fn locale_id(tag: &str) -> u32 {
    let wide: Vec<_> = tag.encode_utf16().chain(Some(0)).collect();
    unsafe { windows_sys::Win32::Globalization::LocaleNameToLCID(wide.as_ptr(), 0) }
}
#[cfg(not(windows))]
fn locale_id(_tag: &str) -> u32 {
    0
}
