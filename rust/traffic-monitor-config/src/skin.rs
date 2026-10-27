use crate::{IniDocument, decode_bytes};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontInfo {
    pub name: String,
    pub size: i32,
    pub style: u8,
}
impl Default for FontInfo {
    fn default() -> Self {
        Self {
            name: "Segoe UI".into(),
            size: 10,
            style: 0,
        }
    }
}
impl FontInfo {
    pub fn load(ini: &IniDocument, section: &str, default: &Self) -> Self {
        Self {
            name: ini.get_string(section, "font_name", &default.name),
            size: ini.get_i64(section, "font_size", default.size.into()) as i32,
            style: ini.get_i64(section, "font_style", default.style.into()) as u8,
        }
    }
    pub fn save(&self, ini: &mut IniDocument, section: &str) {
        ini.set(section, "font_name", self.name.clone());
        ini.set_i64(section, "font_size", self.size.into());
        ini.set_i64(section, "font_style", self.style.into());
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Left,
    Right,
    Center,
    Side,
}
impl Alignment {
    fn from_i64(i: i64) -> Self {
        match i {
            1 => Self::Right,
            2 => Self::Center,
            3 => Self::Side,
            _ => Self::Left,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct LayoutItem {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub align: Alignment,
    pub show: bool,
    pub plugin_id: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub width: i32,
    pub height: i32,
    pub items: BTreeMap<String, LayoutItem>,
}
#[derive(Clone, Debug, Default)]
pub struct PreviewInfo {
    pub width: i32,
    pub height: i32,
    pub large_position: [i32; 2],
    pub small_position: [i32; 2],
}
#[derive(Clone, Debug, Default)]
pub struct Skin {
    pub path: PathBuf,
    pub name: String,
    pub author: String,
    pub font: FontInfo,
    pub colors: Vec<u32>,
    pub specify_each_item_color: bool,
    pub labels: BTreeMap<String, String>,
    pub small: Layout,
    pub large: Layout,
    pub preview: PreviewInfo,
    pub text_height: i32,
    pub no_label: bool,
    pub plugin_map: BTreeMap<String, String>,
    pub background_small: Option<PathBuf>,
    pub background_large: Option<PathBuf>,
    pub mask_small: Option<PathBuf>,
    pub mask_large: Option<PathBuf>,
    pub is_png: bool,
}
#[derive(Clone, Debug)]
pub struct SkinEntry {
    pub name: String,
    pub path: PathBuf,
}
#[derive(Clone, Debug)]
pub struct SkinCatalog {
    pub entries: Vec<SkinEntry>,
}
impl SkinCatalog {
    pub fn load(root: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            entries: enumerate_skins(root)?,
        })
    }
    pub fn select(&self, ini: &IniDocument, light_theme: bool) -> io::Result<Skin> {
        let selected = ini.get_string("config", "skin_selected", "");
        let theme = ini.get_string(
            "skins",
            if light_theme {
                "skin_name_light_mode"
            } else {
                "skin_name_dark_mode"
            },
            "",
        );
        let requested = normalize_skin_name(
            if ini.get_bool("skins", "skin_auto_adapt", false) && !theme.is_empty() {
                &theme
            } else {
                &selected
            },
        );
        let entry = if requested.is_empty() {
            self.entries.first()
        } else {
            self.entries.iter().find(|e| e.name == requested)
        }
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("skin not found: {requested}"),
            )
        })?;
        let mut skin = Skin::load(&entry.path)?;
        skin.apply_overrides(ini);
        Ok(skin)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Bmp,
}
#[derive(Clone, Debug)]
pub struct ImageResource {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
}
impl ImageResource {
    /// Read actual resource bytes and image dimensions. Pixel decoding is owned by the renderer.
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let bytes = fs::read(path)?;
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid skin image: {}", path.display()),
            )
        };
        let (format, width, height) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            && bytes.len() >= 33
            && bytes.get(12..16) == Some(b"IHDR")
        {
            (
                ImageFormat::Png,
                u32::from_be_bytes(bytes[16..20].try_into().map_err(|_| invalid())?),
                u32::from_be_bytes(bytes[20..24].try_into().map_err(|_| invalid())?),
            )
        } else if bytes.starts_with(b"BM") && bytes.len() >= 26 {
            let header = u32::from_le_bytes(bytes[14..18].try_into().map_err(|_| invalid())?);
            if header == 12 {
                (
                    ImageFormat::Bmp,
                    u32::from(u16::from_le_bytes([bytes[18], bytes[19]])),
                    u32::from(u16::from_le_bytes([bytes[20], bytes[21]])),
                )
            } else if header >= 40 && bytes.len() >= 54 {
                (
                    ImageFormat::Bmp,
                    i32::from_le_bytes(bytes[18..22].try_into().map_err(|_| invalid())?)
                        .unsigned_abs(),
                    i32::from_le_bytes(bytes[22..26].try_into().map_err(|_| invalid())?)
                        .unsigned_abs(),
                )
            } else {
                return Err(invalid());
            }
        } else {
            return Err(invalid());
        };
        if width == 0 || height == 0 {
            return Err(invalid());
        }
        Ok(Self {
            path: path.to_owned(),
            bytes,
            format,
            width,
            height,
        })
    }
}
pub fn enumerate_skins(root: impl AsRef<Path>) -> io::Result<Vec<SkinEntry>> {
    let mut skins = Vec::new();
    for e in fs::read_dir(root)? {
        let path = e?.path();
        if path.is_dir() && (path.join("skin.xml").is_file() || path.join("skin.ini").is_file()) {
            skins.push(SkinEntry {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path,
            });
        }
    }
    skins.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skins)
}
pub fn normalize_skin_name(name: &str) -> String {
    name.replace('\\', "/")
        .trim_start_matches("./skins/")
        .trim_matches('/')
        .to_owned()
}
impl Skin {
    pub fn background(&self, more: bool) -> io::Result<Option<ImageResource>> {
        let path = if more {
            &self.background_large
        } else {
            &self.background_small
        };
        path.as_ref().map(ImageResource::load).transpose()
    }
    pub fn mask(&self, more: bool) -> io::Result<Option<ImageResource>> {
        let path = if more {
            &self.mask_large
        } else {
            &self.mask_small
        };
        path.as_ref().map(ImageResource::load).transpose()
    }
    pub fn load(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref();
        let mut skin = if dir.join("skin.xml").is_file() {
            Self::from_xml(&decode_bytes(&fs::read(dir.join("skin.xml"))?)?)?
        } else {
            Self::from_ini(&IniDocument::load(dir.join("skin.ini"))?)
        };
        skin.path = dir.to_owned();
        skin.name = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        skin.is_png =
            dir.join("background.png").is_file() && dir.join("background_l.png").is_file();
        let ext = if skin.is_png { "png" } else { "bmp" };
        skin.background_small = existing(dir.join(format!("background.{ext}")));
        skin.background_large = existing(dir.join(format!("background_l.{ext}")));
        skin.mask_small = existing(dir.join("mask.bmp"));
        skin.mask_large = existing(dir.join("mask_l.bmp"));
        if skin.small.width <= 0
            || skin.small.height <= 0
            || skin.large.width <= 0
            || skin.large.height <= 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "skin dimensions must be positive",
            ));
        }
        Ok(skin)
    }
    pub fn color(&self, index: usize) -> u32 {
        self.colors
            .get(if self.specify_each_item_color {
                index
            } else {
                0
            })
            .or(self.colors.first())
            .copied()
            .unwrap_or(0)
    }
    pub fn layout(&self, more: bool) -> &Layout {
        if more { &self.large } else { &self.small }
    }
    pub fn scaled_layout(&self, more: bool, dpi: u32) -> Layout {
        let scale = |v: i32| {
            ((i64::from(v) * i64::from(dpi) + 48) / 96).clamp(i32::MIN as i64, i32::MAX as i64)
                as i32
        };
        let mut layout = self.layout(more).clone();
        layout.width = scale(layout.width);
        layout.height = scale(layout.height);
        for i in layout.items.values_mut() {
            i.x = scale(i.x);
            i.y = scale(i.y);
            i.width = scale(i.width);
        }
        layout
    }
    pub fn apply_overrides(&mut self, ini: &IniDocument) {
        let section = format!("skin_{}", self.name);
        self.font = FontInfo::load(ini, &section, &self.font);
        if let Some(colors) = ini.get(&section, "text_color") {
            self.colors = parse_colors(colors);
        }
        self.specify_each_item_color = ini.get_bool(
            &section,
            "specify_each_item_color",
            self.specify_each_item_color,
        );
        for (s, k, v) in ini.entries() {
            if s == section {
                if let Some(item) = crate::DISPLAY_ITEMS.iter().find(|i| i.ini_key == k) {
                    self.labels.insert(item.id.into(), v.into());
                } else if let Some((node, _)) =
                    self.plugin_map.iter().find(|(_, id)| id.as_str() == k)
                {
                    self.labels.insert(node.clone(), v.into());
                }
            }
        }
    }
    pub fn save_overrides(&self, ini: &mut IniDocument) {
        let s = format!("skin_{}", self.name);
        self.font.save(ini, &s);
        ini.set(
            &s,
            "text_color",
            self.colors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        ini.set_bool(&s, "specify_each_item_color", self.specify_each_item_color);
        for (k, v) in &self.labels {
            if let Some(item) = crate::DISPLAY_ITEMS.iter().find(|i| i.id == k) {
                ini.set(&s, item.ini_key, v.clone());
            } else if let Some(id) = self.plugin_map.get(k) {
                ini.set(&s, id, v.clone());
            }
        }
    }
    pub fn from_ini(ini: &IniDocument) -> Self {
        let mut skin = Self {
            font: FontInfo::load(ini, "skin", &FontInfo::default()),
            author: ini.get_string("skin", "skin_author", "unknow"),
            colors: parse_colors(&ini.get_string("skin", "text_color", "0")),
            specify_each_item_color: ini.get_bool("skin", "specify_each_item_color", false),
            text_height: ini.get_i64("layout", "text_height", 20) as i32,
            no_label: ini.get_bool("layout", "no_text", false),
            ..Self::default()
        };
        for item in ["up", "down", "cpu", "memory"] {
            if let Some(v) = ini.get("skin", &format!("{item}_string")) {
                skin.labels.insert(item.into(), v.into());
            }
        }
        skin.small = ini_layout(ini, "s");
        skin.large = ini_layout(ini, "l");
        skin.preview = PreviewInfo {
            width: ini.get_i64("layout", "preview_width", 238) as i32,
            height: ini.get_i64("layout", "preview_height", 105) as i32,
            small_position: [
                ini.get_i64("layout", "preview_x_s", 0) as i32,
                ini.get_i64("layout", "preview_y_s", 0) as i32,
            ],
            large_position: [
                ini.get_i64("layout", "preview_x_l", 0) as i32,
                ini.get_i64("layout", "preview_y_l", 47) as i32,
            ],
        };
        skin
    }
    pub fn from_xml(text: &str) -> io::Result<Self> {
        let root = parse_xml(text)?;
        if root.name != "root" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "skin XML requires root",
            ));
        }
        let mut skin = Self::default();
        if let Some(n) = root.child("skin") {
            skin.author = n.child_text("skin_author");
            skin.colors = parse_colors(&n.child_text("text_color"));
            skin.specify_each_item_color = n
                .child_text("specify_each_item_color")
                .parse::<i64>()
                .unwrap_or(0)
                != 0;
            if let Some(f) = n.child("font") {
                skin.font = FontInfo {
                    name: f.attr("name").into(),
                    size: f.number("size") as i32,
                    style: f.number("style") as u8,
                };
            }
            if let Some(t) = n.child("display_text") {
                for n in &t.children {
                    skin.labels.insert(n.name.clone(), n.text.clone());
                }
            }
        }
        if let Some(m) = root.child("plugin_map") {
            for n in &m.children {
                skin.plugin_map.insert(n.name.clone(), n.text.trim().into());
            }
        }
        let layout = root
            .child("layout")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "skin XML missing layout"))?;
        skin.text_height = layout.number("text_height") as i32;
        skin.no_label = layout.number("no_label") != 0;
        if let Some(n) = layout.child("layout_s") {
            skin.small = xml_layout(n, &skin.plugin_map);
        }
        if let Some(n) = layout.child("layout_l") {
            skin.large = xml_layout(n, &skin.plugin_map);
        }
        if let Some(p) = root.child("preview") {
            skin.preview.width = p.number("width") as i32;
            skin.preview.height = p.number("height") as i32;
            for n in &p.children {
                let pos = [n.number("x") as i32, n.number("y") as i32];
                if n.name == "s" {
                    skin.preview.small_position = pos;
                } else if n.name == "l" {
                    skin.preview.large_position = pos;
                }
            }
        }
        Ok(skin)
    }
}
fn existing(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}
fn parse_colors(v: &str) -> Vec<u32> {
    v.split(',')
        .filter_map(|c| c.trim().parse::<u32>().ok())
        .collect()
}
fn ini_layout(ini: &IniDocument, suffix: &str) -> Layout {
    let large = suffix == "l";
    let mut layout = Layout {
        width: ini.get_i64("layout", &format!("width_{suffix}"), 220) as i32,
        height: ini.get_i64(
            "layout",
            &format!("height_{suffix}"),
            if large { 43 } else { 28 },
        ) as i32,
        ..Layout::default()
    };
    for (index, item) in ["up", "down", "cpu", "memory"].iter().enumerate() {
        let get = |field: &str, default| {
            ini.get_i64("layout", &format!("{item}_{field}_{suffix}"), default)
        };
        layout.items.insert(
            (*item).into(),
            LayoutItem {
                x: get("x", if index % 2 == 0 { 6 } else { 114 }) as i32,
                y: get(
                    "y",
                    if large {
                        if index < 2 { 2 } else { 21 }
                    } else {
                        4
                    },
                ) as i32,
                width: get("width", if index % 2 == 0 { 108 } else { 110 }) as i32,
                align: Alignment::from_i64(get("align", 0)),
                show: ini.get_bool(
                    "layout",
                    &format!("show_{item}_{suffix}"),
                    large || index < 2,
                ),
                plugin_id: None,
            },
        );
    }
    layout
}
#[derive(Default)]
struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    text: String,
    children: Vec<Node>,
}
impl Node {
    fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|n| n.name == name)
    }
    fn child_text(&self, name: &str) -> String {
        self.child(name).map(|n| n.text.clone()).unwrap_or_default()
    }
    fn attr(&self, name: &str) -> &str {
        self.attrs.get(name).map(String::as_str).unwrap_or("")
    }
    fn number(&self, name: &str) -> i64 {
        match self.attr(name) {
            "true" => 1,
            v => v.parse().unwrap_or(0),
        }
    }
}
fn parse_xml(text: &str) -> io::Result<Node> {
    use quick_xml::{Reader, events::Event};
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let empty = text
                    .as_bytes()
                    .get(reader.buffer_position().saturating_sub(2) as usize)
                    == Some(&b'/');
                let mut node = Node {
                    name: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
                    ..Node::default()
                };
                for attr in e.attributes() {
                    let attr = attr.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                    node.attrs.insert(
                        String::from_utf8_lossy(attr.key.as_ref()).into_owned(),
                        attr.decode_and_unescape_value(reader.decoder())
                            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
                            .into_owned(),
                    );
                }
                if empty {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else if root.replace(node).is_some() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "multiple XML roots",
                        ));
                    }
                } else {
                    if stack.len() > 128 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "skin XML nesting limit",
                        ));
                    }
                    stack.push(node);
                }
            }
            Event::Text(e) => {
                if let Some(n) = stack.last_mut() {
                    n.text.push_str(
                        &quick_xml::escape::unescape(&e.decode().map_err(io::Error::other)?)
                            .map_err(io::Error::other)?,
                    );
                }
            }
            Event::CData(e) => {
                if let Some(n) = stack.last_mut() {
                    n.text.push_str(&e.decode().map_err(io::Error::other)?);
                }
            }
            Event::GeneralRef(e) => {
                if let Some(n) = stack.last_mut() {
                    let name = e.decode().map_err(io::Error::other)?;
                    let escaped = format!("&{name};");
                    n.text.push_str(
                        &quick_xml::escape::unescape(&escaped).map_err(io::Error::other)?,
                    );
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "unexpected XML end")
                })?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else if root.replace(node).is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "multiple XML roots",
                    ));
                }
            }
            Event::DocType(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "skin DTD is unsupported",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unclosed skin XML",
        ));
    }
    root.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty skin XML"))
}
fn xml_layout(n: &Node, plugins: &BTreeMap<String, String>) -> Layout {
    let mut layout = Layout {
        width: n.number("width") as i32,
        height: n.number("height") as i32,
        ..Layout::default()
    };
    for item in &n.children {
        layout.items.insert(
            item.name.clone(),
            LayoutItem {
                x: item.number("x") as i32,
                y: item.number("y") as i32,
                width: item.number("width") as i32,
                align: Alignment::from_i64(item.number("align")),
                show: item.number("show") != 0,
                plugin_id: plugins.get(&item.name).cloned(),
            },
        );
    }
    layout
}
