use crate::{IniDocument, Language};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpeedUnit {
    #[default]
    Auto,
    Kilobytes,
    Megabytes,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemoryDisplay {
    #[default]
    Percentage,
    Used,
    Available,
}
#[derive(Clone, Debug)]
pub struct FormatOptions {
    pub unit_byte: bool,
    pub speed_unit: SpeedUnit,
    pub short: bool,
    pub space: bool,
    pub hide_unit: bool,
    pub hide_percent: bool,
    pub memory_display: MemoryDisplay,
    pub precision: Option<usize>,
}
impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            unit_byte: true,
            speed_unit: SpeedUnit::Auto,
            short: false,
            space: true,
            hide_unit: false,
            hide_percent: false,
            memory_display: MemoryDisplay::Percentage,
            precision: None,
        }
    }
}
impl FormatOptions {
    pub fn from_ini(ini: &IniDocument, taskbar: bool) -> Self {
        let s = if taskbar { "task_bar" } else { "config" };
        let key = |key: &str| {
            if taskbar
                && matches!(
                    key,
                    "speed_short_mode" | "speed_unit" | "hide_unit" | "hide_percent"
                )
            {
                format!("task_bar_{key}")
            } else {
                key.into()
            }
        };
        Self {
            unit_byte: ini.get_bool(s, "unit_byte", true),
            speed_unit: match ini.get_i64(s, &key("speed_unit"), 0) {
                1 => SpeedUnit::Kilobytes,
                2 => SpeedUnit::Megabytes,
                _ => SpeedUnit::Auto,
            },
            short: ini.get_bool(s, &key("speed_short_mode"), taskbar),
            space: ini.get_bool(s, "separate_value_unit_with_space", true),
            hide_unit: ini.get_bool(s, &key("hide_unit"), false),
            hide_percent: ini.get_bool(s, &key("hide_percent"), false),
            memory_display: match ini.get_i64(s, "memory_display", 0) {
                1 => MemoryDisplay::Used,
                2 => MemoryDisplay::Available,
                _ => MemoryDisplay::Percentage,
            },
            precision: None,
        }
    }
    fn join(&self, value: String, unit: &str) -> String {
        if unit.is_empty() {
            value
        } else {
            format!("{value}{}{unit}", if self.space { " " } else { "" })
        }
    }
    pub fn data_size(&self, bytes: f64) -> String {
        if !bytes.is_finite() || bytes < 0.0 {
            return "--".into();
        }
        let value = bytes * if self.unit_byte { 1.0 } else { 8.0 };
        let (scale, unit, precision) = match self.speed_unit {
            SpeedUnit::Auto => {
                if value < 10240.0 {
                    (1024.0, "K", if self.short { 1 } else { 2 })
                } else if value < 1024000.0 {
                    (1024.0, "K", if self.short { 0 } else { 1 })
                } else if value < 1048576000.0 {
                    (1048576.0, "M", if self.short { 1 } else { 2 })
                } else {
                    (1073741824.0, "G", 2)
                }
            }
            SpeedUnit::Kilobytes => (
                1024.0,
                "K",
                if value < 10240.0 {
                    if self.short { 1 } else { 2 }
                } else if self.short {
                    0
                } else {
                    1
                },
            ),
            SpeedUnit::Megabytes => (1048576.0, "M", if self.short { 1 } else { 2 }),
        };
        let precision = self.precision.unwrap_or(precision).min(9);
        let number = format!("{:.*}", precision, value / scale);
        let unit = if self.hide_unit {
            String::new()
        } else if self.short {
            format!("{unit}{}", if self.unit_byte { "" } else { "b" })
        } else {
            format!("{unit}{}", if self.unit_byte { "B" } else { "b" })
        };
        self.join(number, &unit)
    }
    pub fn speed(&self, bytes_per_second: f64) -> String {
        let value = self.data_size(bytes_per_second);
        if self.hide_unit || value == "--" {
            value
        } else {
            format!("{value}/s")
        }
    }
    pub fn percentage(&self, value: f64) -> String {
        let number = if !value.is_finite() || value < 0.0 {
            "--".into()
        } else {
            format!("{}", value as i64)
        };
        self.join(number, if self.hide_percent { "" } else { "%" })
    }
    pub fn temperature(&self, value: f64) -> String {
        self.join(
            if !value.is_finite() || value <= 0.0 {
                "--".into()
            } else {
                (value as i64).to_string()
            },
            "°C",
        )
    }
    pub fn frequency(&self, ghz: f64) -> String {
        self.join(
            if !ghz.is_finite() || ghz < 0.0 {
                "--".into()
            } else {
                format!("{ghz:.2}")
            },
            "GHz",
        )
    }
    pub fn memory(&self, total_bytes: u64, available_bytes: u64) -> String {
        let available = available_bytes.min(total_bytes);
        match self.memory_display {
            MemoryDisplay::Percentage => self.percentage(if total_bytes == 0 {
                -1.0
            } else {
                100.0 * (total_bytes - available) as f64 / total_bytes as f64
            }),
            MemoryDisplay::Used => format_bytes((total_bytes - available) as f64, self.space),
            MemoryDisplay::Available => format_bytes(available as f64, self.space),
        }
    }
    pub fn graph_fraction(&self, value: f64, maximum: f64) -> f64 {
        if !value.is_finite() || !maximum.is_finite() || maximum <= 0.0 {
            0.0
        } else {
            (value / maximum).clamp(0.0, 1.0)
        }
    }
}
pub fn format_bytes(bytes: f64, space: bool) -> String {
    if !bytes.is_finite() || bytes < 0.0 {
        return "--".into();
    }
    let (scale, unit, precision) = if bytes < 10240.0 {
        (1024.0, "KB", 2)
    } else if bytes < 1048576.0 {
        (1024.0, "KB", 1)
    } else if bytes < 1073741824.0 {
        (1048576.0, "MB", 2)
    } else if bytes < 1099511627776.0 {
        (1073741824.0, "GB", 2)
    } else {
        (1099511627776.0, "TB", 2)
    };
    format!(
        "{:.*}{}{unit}",
        precision,
        bytes / scale,
        if space { " " } else { "" }
    )
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayItemInfo {
    pub id: &'static str,
    pub ini_key: &'static str,
    pub language_key: &'static str,
    pub fallback_label: &'static str,
    pub index: u8,
}
pub const DISPLAY_ITEMS: [DisplayItemInfo; 13] = [
    DisplayItemInfo {
        id: "up",
        ini_key: "up_string",
        language_key: "IDS_UPLOAD_DISP",
        fallback_label: "Upload",
        index: 0,
    },
    DisplayItemInfo {
        id: "down",
        ini_key: "down_string",
        language_key: "IDS_DOWNLOAD_DISP",
        fallback_label: "Download",
        index: 1,
    },
    DisplayItemInfo {
        id: "cpu",
        ini_key: "cpu_string",
        language_key: "",
        fallback_label: "CPU",
        index: 2,
    },
    DisplayItemInfo {
        id: "memory",
        ini_key: "memory_string",
        language_key: "IDS_MEMORY_DISP",
        fallback_label: "Memory",
        index: 3,
    },
    DisplayItemInfo {
        id: "gpu",
        ini_key: "gpu_string",
        language_key: "IDS_GPU_DISP",
        fallback_label: "GPU",
        index: 4,
    },
    DisplayItemInfo {
        id: "cpu_temperature",
        ini_key: "cpu_temp_string",
        language_key: "IDS_CPU_TEMPERATURE",
        fallback_label: "CPU temperature",
        index: 5,
    },
    DisplayItemInfo {
        id: "gpu_temperature",
        ini_key: "gpu_temp_string",
        language_key: "IDS_GPU_DISP",
        fallback_label: "GPU",
        index: 6,
    },
    DisplayItemInfo {
        id: "hdd_temperature",
        ini_key: "hdd_temp_string",
        language_key: "IDS_HDD_DISP",
        fallback_label: "Disk",
        index: 7,
    },
    DisplayItemInfo {
        id: "main_board_temperature",
        ini_key: "main_board_temp_string",
        language_key: "IDS_MAINBOARD_DISP",
        fallback_label: "Motherboard",
        index: 8,
    },
    DisplayItemInfo {
        id: "hdd",
        ini_key: "hdd_string",
        language_key: "IDS_HDD_DISP",
        fallback_label: "Disk",
        index: 9,
    },
    DisplayItemInfo {
        id: "total_speed",
        ini_key: "total_speed_string",
        language_key: "IDS_TOTAL_SPEED",
        fallback_label: "Total speed",
        index: 10,
    },
    DisplayItemInfo {
        id: "cpu_freq",
        ini_key: "cpu_freq_string",
        language_key: "IDS_CPU_FREQ",
        fallback_label: "CPU frequency",
        index: 11,
    },
    DisplayItemInfo {
        id: "today_traffic",
        ini_key: "today_traffic_string",
        language_key: "IDS_TRAFFIC_USED",
        fallback_label: "Today traffic",
        index: 12,
    },
];
pub fn display_item_label(id: &str, language: Option<&Language>, main_window: bool) -> String {
    let Some(item) = DISPLAY_ITEMS.iter().find(|item| item.id == id) else {
        return id.into();
    };
    let name = language
        .map(|l| l.text("text", item.language_key, item.fallback_label))
        .unwrap_or_else(|| item.fallback_label.into());
    match (id, main_window) {
        ("up", false) => "↑: ".into(),
        ("down", false) => "↓: ".into(),
        ("total_speed", _) => "↑↓: ".into(),
        ("cpu_temperature", _) => "CPU: ".into(),
        _ => format!("{name}: "),
    }
}
/// Legacy item order contains numeric indices separated by comma; omitted items follow.
pub fn display_order(value: &str) -> Vec<&'static str> {
    let mut ordered = Vec::new();
    for index in value.split(',').filter_map(|v| v.trim().parse::<u8>().ok()) {
        if let Some(item) = DISPLAY_ITEMS.iter().find(|item| item.index == index) {
            if !ordered.contains(&item.id) {
                ordered.push(item.id);
            }
        }
    }
    for item in DISPLAY_ITEMS {
        if !ordered.contains(&item.id) {
            ordered.push(item.id);
        }
    }
    ordered
}
