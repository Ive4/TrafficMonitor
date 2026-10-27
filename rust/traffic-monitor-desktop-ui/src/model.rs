//! Backend-neutral desktop contract. Colors are Win32 COLORREF (0x00BBGGRR).
use std::path::PathBuf;
#[derive(Clone, Debug, PartialEq)]
pub struct DesktopOptions {
    pub title: String,
    pub resource_dir: PathBuf,
    pub show_main: bool,
    pub show_taskbar: bool,
    pub show_tray: bool,
    pub tray_icon: Option<PathBuf>,
    pub history_log_scale: bool,
    pub topmost: bool,
    pub click_through: bool,
    pub lock_position: bool,
    pub snap_to_edges: bool,
    pub hide_fullscreen: bool,
    pub tooltip: bool,
    pub swap_up_down: bool,
    pub opacity: u8,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub font_name: String,
    pub font_size: i32,
    pub font_style: u8,
    pub foreground: u32,
    pub background: u32,
    pub interval_ms: u32,
    pub dark: bool,
    pub follow_system_theme: bool,
    pub taskbar_monitor: usize,
    pub taskbar_width: i32,
    pub taskbar_offset: i32,
    pub taskbar_two_rows: bool,
    pub item_gap: i32,
    pub taskbar_style: TaskbarStyle,
    pub visible_items: Vec<String>,
    pub skin: Option<SkinLayout>,
    pub double_click: DoubleClickAction,
    pub taskbar_double_click: DoubleClickAction,
    /// Explicit harness timeout. Production should leave this None.
    pub exit_after_ms: Option<u32>,
}
impl Default for DesktopOptions {
    fn default() -> Self {
        Self {
            title: "TrafficMonitor".into(),
            resource_dir: PathBuf::new(),
            show_main: true,
            show_taskbar: false,
            show_tray: true,
            tray_icon: None,
            history_log_scale: false,
            topmost: true,
            click_through: false,
            lock_position: false,
            snap_to_edges: true,
            hide_fullscreen: true,
            tooltip: true,
            swap_up_down: false,
            opacity: 240,
            x: 40,
            y: 40,
            width: 300,
            height: 100,
            font_name: "Segoe UI".into(),
            font_size: 14,
            font_style: 0,
            foreground: 0x00eeeeee,
            background: 0x00251e18,
            interval_ms: 1000,
            dark: true,
            follow_system_theme: false,
            taskbar_monitor: 0,
            taskbar_width: 180,
            taskbar_offset: 0,
            taskbar_two_rows: true,
            item_gap: 8,
            taskbar_style: TaskbarStyle::default(),
            visible_items: Vec::new(),
            skin: None,
            double_click: DoubleClickAction::Settings,
            taskbar_double_click: DoubleClickAction::Settings,
            exit_after_ms: None,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DoubleClickAction {
    #[default]
    Settings,
    History,
    Network,
    TaskManager,
    ToggleMain,
    ToggleMore,
    SpecificApp,
    ChangeSkin,
    None,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SkinLayout {
    pub background: Option<PathBuf>,
    pub mask: Option<PathBuf>,
    pub width: i32,
    pub height: i32,
    pub items: Vec<SkinItem>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SkinItem {
    pub id: String,
    pub rect: [i32; 4],
    pub color: Option<u32>,
    pub align_right: bool,
    pub alignment: TextAlignment,
    pub show_label: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlignment {
    #[default]
    Left,
    Right,
    Center,
    Side,
}
#[derive(Clone, Debug, Default)]
pub struct DisplayValue {
    pub id: String,
    pub label: String,
    pub value: String,
    pub fraction: Option<f64>,
    pub history: Vec<f64>,
    pub color: Option<u32>,
    pub plugin: bool,
    pub custom_draw: bool,
    pub width: Option<i32>,
    pub double_line: bool,
}
#[derive(Clone, Debug, Default)]
pub struct NetworkInfo {
    pub id: String,
    pub name: String,
    pub selected: bool,
    pub details: Vec<(String, String)>,
}
#[derive(Clone, Debug, Default)]
pub struct HistoryRow {
    pub date: String,
    pub upload: String,
    pub download: String,
    pub total: String,
    pub mixed: String,
    pub total_bytes: u128,
}
#[derive(Clone, Debug, Default)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub description: String,
    pub commands: Vec<(u32, String)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingKind {
    Bool,
    Text,
    Integer,
    Color,
    Font,
    List,
    Choice(Vec<String>),
}
#[derive(Clone, Debug)]
pub struct SettingField {
    pub key: String,
    pub label: String,
    pub value: String,
    pub kind: SettingKind,
    pub help: String,
}
#[derive(Clone, Debug, Default)]
pub struct Choice {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, Default)]
pub struct Dashboard {
    pub current_skin: String,
    pub current_language: String,
    pub connection_mode: ConnectionMode,
    pub history_grouping: HistoryGrouping,
    pub history_from: String,
    pub history_to: String,
    pub auto_start_enabled: Option<bool>,
    pub auto_start_scheduler: bool,
    pub items: Vec<DisplayValue>,
    pub taskbar_items: Vec<DisplayValue>,
    pub tooltip: String,
    pub status: String,
    pub networks: Vec<NetworkInfo>,
    pub history: Vec<HistoryRow>,
    pub history_summary: String,
    pub plugins: Vec<PluginInfo>,
    pub settings: Vec<SettingField>,
    pub skins: Vec<Choice>,
    pub languages: Vec<Choice>,
    pub notices: Vec<String>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryGrouping {
    #[default]
    Day,
    Week,
    Month,
    Quarter,
    Year,
}
#[derive(Clone, Debug, PartialEq)]
// Actions are low-frequency user commands; keep the public options value ergonomic.
#[allow(clippy::large_enum_variant)]
pub enum DesktopAction {
    ApplyPreferences {
        options: DesktopOptions,
        fields: Vec<(String, String)>,
        skin: Option<String>,
        language: Option<String>,
        auto_start: Option<bool>,
    },
    ApplyOptions(DesktopOptions),
    SaveSettings(Vec<(String, String)>),
    SelectConnections {
        mode: ConnectionMode,
        ids: Vec<String>,
    },
    HistoryRange {
        from: String,
        to: String,
        grouping: HistoryGrouping,
    },
    ImportHistory(PathBuf),
    ExportHistory(PathBuf),
    SelectSkin(String),
    SelectLanguage(String),
    PluginEnabled {
        id: String,
        enabled: bool,
    },
    PluginConfigure(String),
    PluginCommand {
        id: String,
        command: u32,
    },
    SetAutoStart(bool),
    CheckUpdates,
    OpenTaskManager,
    ToggleMore,
    OpenSpecificApp {
        taskbar: bool,
    },
    CycleSkin,
    OpenDataDirectory,
    ResetPosition,
    PositionChanged {
        x: i32,
        y: i32,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnectionMode {
    #[default]
    Auto,
    All,
    Manual,
}
pub trait DesktopBackend {
    fn options(&self) -> DesktopOptions;
    fn sample(&mut self) -> Result<Dashboard, String>;
    fn dispatch(&mut self, action: DesktopAction) -> Result<(), String>;
    fn attach_windows(&mut self, _main: usize, _taskbar: Option<usize>) {}
    fn text(&self, _key: &str, fallback: &str) -> String {
        fallback.to_owned()
    }
    fn locale(&self) -> String {
        "en-US".into()
    }
    /// Flush persistent state without stopping sampling or unloading plugins.
    fn checkpoint(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn draw_plugin(
        &mut self,
        _id: &str,
        _hdc: usize,
        _rect: [i32; 4],
        _dark: bool,
        _taskbar: bool,
    ) -> bool {
        false
    }
    /// Native client coordinates and ABI mouse kind: left=0, right=1, double=2, wheel up=3, down=4.
    fn plugin_mouse(&mut self, _id: &str, _event: PluginMouseEvent) -> bool {
        false
    }
    fn plugin_width(&mut self, _id: &str, _hdc: usize, _taskbar: bool) -> Option<i32> {
        None
    }
    fn plugin_keyboard(&mut self, _id: &str, _event: PluginKeyboardEvent) -> bool {
        false
    }
    fn shutdown(&mut self) -> Result<(), String> {
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct PluginMouseEvent {
    pub kind: i32,
    pub x: i32,
    pub y: i32,
    pub window: usize,
    pub taskbar: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct PluginKeyboardEvent {
    pub key: i32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub window: usize,
    pub taskbar: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskbarStyle {
    pub auto_width: bool,
    pub value_digits: u8,
    pub visible_items: Vec<String>,
    pub font_name: String,
    pub font_size: i32,
    pub font_style: u8,
    pub foreground: u32,
    pub background: u32,
    pub opacity: u8,
    pub auto_background: bool,
    pub disable_d2d: bool,
    pub colorful_emoji: bool,
    pub transparent_color: Option<u32>,
    pub top_offset: i32,
    pub vertical_margin: i32,
    pub graph_maximum: Option<f64>,
    pub graph_color: Option<u32>,
    pub graph_style: GraphStyle,
    pub item_colors: Vec<ItemColors>,
    pub swap_up_down: bool,
    pub label_color: Option<u32>,
    pub value_color: Option<u32>,
    pub align_right: bool,
    pub show_labels: bool,
    pub show_graph: bool,
    pub show_status_bar: bool,
    pub tooltip: bool,
    pub place_left: bool,
    pub snap_to_icons: bool,
    pub show_graph_dashed_box: bool,
    pub avoid_widgets: bool,
    pub left_space: i32,
    pub right_space: i32,
}
impl Default for TaskbarStyle {
    fn default() -> Self {
        Self {
            auto_width: true,
            value_digits: 4,
            visible_items: Vec::new(),
            font_name: "Segoe UI".into(),
            font_size: 12,
            font_style: 0,
            foreground: 0x00eeeeee,
            background: 0x00251e18,
            opacity: 255,
            auto_background: false,
            disable_d2d: false,
            colorful_emoji: true,
            transparent_color: None,
            top_offset: 0,
            vertical_margin: 0,
            graph_maximum: None,
            graph_color: None,
            graph_style: GraphStyle::Line,
            item_colors: vec![],
            swap_up_down: false,
            label_color: None,
            value_color: None,
            align_right: false,
            show_labels: true,
            show_graph: true,
            show_status_bar: true,
            tooltip: true,
            place_left: false,
            snap_to_icons: false,
            show_graph_dashed_box: false,
            avoid_widgets: true,
            left_space: 160,
            right_space: 88,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GraphStyle {
    #[default]
    Line,
    Bar,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemColors {
    pub id: String,
    pub label: Option<u32>,
    pub value: Option<u32>,
}
