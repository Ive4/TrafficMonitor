use crate::{DISPLAY_ITEMS, IniDocument, Language, LanguageCatalog};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingKind {
    Boolean,
    Integer,
    Text,
    Choice,
    Color,
    Font,
    List,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingOption {
    pub value: String,
    pub label: String,
}
#[derive(Clone, Debug)]
pub struct SettingSpec {
    pub section: &'static str,
    pub key: &'static str,
    pub label: &'static str,
    pub kind: SettingKind,
    pub default: &'static str,
    pub options: Vec<SettingOption>,
    pub min: Option<i64>,
    pub max: Option<i64>,
}
#[derive(Clone, Debug)]
pub struct LocalizedSettingSpec {
    pub section: &'static str,
    pub key: &'static str,
    pub label: String,
    pub kind: SettingKind,
    pub default: &'static str,
    pub options: Vec<SettingOption>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    source: SettingSpec,
    validation_message: String,
}
impl LocalizedSettingSpec {
    pub fn value(&self, ini: &IniDocument) -> String {
        self.source.value(ini)
    }
    pub fn validate(&self, value: &str) -> Result<String, String> {
        self.source
            .validate(value)
            .map_err(|_| format!("{}: {}", self.label, self.validation_message))
    }
}
fn pack_label(language: &Language, key: &str, fallback: &str) -> String {
    let value = language.text("text", key, "");
    let value = if value.is_empty() {
        language.text("menu", key, fallback)
    } else {
        value
    };
    value
        .replace('&', "")
        .trim()
        .trim_end_matches(':')
        .to_owned()
}
fn english_label(key: &str) -> String {
    let specific = match key {
        "alow_out_of_border" => "Allow window outside screen boundaries",
        "taksbar_transparent_color_enable" => "Enable taskbar transparent color",
        "cpu_core_name" => "CPU temperature sensor",
        "hardware_monitor_item" => "Hardware monitor items",
        "monitor_time_span" => "Monitoring interval (milliseconds)",
        "taskbar_left_space_win11" => "Windows 11 taskbar left reserved space",
        "taskbar_right_space_win11" => "Windows 11 taskbar right reserved space",
        "tbar_display_item" => "Taskbar display items",
        "cm_graph_type" => "Graph display mode",
        "notify_icon_selected" => "Notification icon style",
        "traffic_tip_value" => "Traffic notification threshold",
        "traffic_tip_unit" => "Traffic notification unit",
        "memory_tip_value" => "Memory usage threshold (%)",
        "notify_interval" => "Notification cooldown (seconds)",
        "no_multistart_warning" => "Suppress the already-running warning",
        "exit_when_start_by_restart_manager" => "Exit when launched by Restart Manager",
        "show_dot_net_notinstalled_tip" => "Show hardware component warning",
        "show_mouse_panetrate_tip" => "Show mouse passthrough warning",
        "cpu_temperature_tip_value" => "CPU temperature threshold (°C)",
        "gpu_temperature_tip_value" => "GPU temperature threshold (°C)",
        "hdd_temperature_tip_value" => "Disk temperature threshold (°C)",
        "mainboard_temperature_tip_value" => "Motherboard temperature threshold (°C)",
        _ => "",
    };
    if !specific.is_empty() {
        return specific.into();
    }
    let key = key.strip_prefix("task_bar_").unwrap_or(key);
    key.split('_')
        .map(|word| match word {
            "cpu" => "CPU".into(),
            "gpu" => "GPU".into(),
            "hdd" => "Disk".into(),
            "d2d" => "Direct2D".into(),
            "exe" => "application".into(),
            "tbar" => "Taskbar".into(),
            "wnd" => "window".into(),
            "disp" => "display".into(),
            "pos" => "position".into(),
            "specify" => "Specify".into(),
            "netspeed" => "network speed".into(),
            word => word.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn label_key(key: &str) -> &str {
    match key {
        "check_update_when_start" => "TXT_CHECK_UPDATE",
        "show_all_interface" => "TXT_SHOW_ALL_CONNECTION_CHECK",
        "auto_run_use_task_scheduler" => "TXT_AUTO_RUN_METHOD_TASK_SCHEDULE",
        "language" => "TXT_LANGUAGE",
        "update_source" => "TXT_UPDATE_SOURCE",
        "cpu_usage_acquire_method" => "TXT_CPU_ACQUISITION_METHOD",
        "monitor_time_span" => "TXT_MONITORING_INTERVALS",
        "hard_disk_name" => "TXT_SELECT_HDD_STATIC",
        "cpu_core_name" => "TXT_SELECT_CPU_STATIC",
        "hardware_monitor_item" => "TXT_HARDWARE_MONITORING",
        "transparency" => "TXT_WINDOW_OPACITY",
        "always_on_top" => "TXT_ALWAYS_ON_TOP",
        "lock_window_pos" => "TXT_LOCK_WINDOW_POS",
        "show_notify_icon" => "TXT_SHOW_NOTIFY_ICON_CHECK",
        "show_cpu_memory" => "TXT_SHOW_MORE_INFO",
        "mouse_penetrate" => "TXT_MOUSE_PENETRATE",
        "show_task_bar_wnd" => "TXT_SHOW_TASKBAR_WINDOW",
        "hide_main_window" => "TXT_SHOW_HIDE_MAIN_WINDOW",
        "skin_selected" => "TXT_CHANGE_SKIN",
        "notify_icon_selected" => "TXT_TITLE_CHANGE_ICON",
        "notify_icon_auto_adapt" => "TXT_NOTIFY_ICON_AUTO_ADAPT_CHECK",
        "swap_up_down" => "TXT_SWITCH_UP_DOWN_CHECK",
        "hide_main_wnd_when_fullscreen" => "TXT_FULLSCREEN_HIDE_CHECK",
        "speed_short_mode" | "task_bar_speed_short_mode" => "TXT_SPEED_SHORT_MODE_CHECK",
        "separate_value_unit_with_space" => "TXT_SEPARATE_VALUE_UNIT_CHECK",
        "show_tool_tip" => "TXT_SHOW_TOOL_TIP_CHK",
        "unit_byte" => "TXT_UNIT_BYTE_RADIO",
        "speed_unit" | "task_bar_speed_unit" => "TXT_NETSPEED_UNIT",
        "hide_unit" | "task_bar_hide_unit" => "TXT_HIDE_UNIT_CHECK",
        "hide_percent" | "task_bar_hide_percent" => "TXT_HIDE_PERCENTAGE_CHECK",
        "double_click_action" => "TXT_DOUBLE_CLICK_ACTION",
        "double_click_exe" => "TXT_EXE_PATH_STATIC",
        "alow_out_of_border" => "TXT_ALLOW_OUT_OF_BOUNDARIES",
        "traffic_tip_enable" => "TXT_TODAY_TRAFFIC_TIP_CHECK",
        "memory_usage_tip_enable" => "TXT_MEMORY_USAGE_TIP_CHECK",
        "cpu_temperature_tip_enable" => "TXT_CPU_TEMP_TIP_CHECK",
        "gpu_temperature_tip_enable" => "TXT_GPU_TEMP_TIP_CHECK",
        "hdd_temperature_tip_enable" => "TXT_HDD_TEMP_TIP_CHECK",
        "mainboard_temperature_tip_enable" => "TXT_MBD_TEMP_TIP_CHECK",
        "task_bar_back_color" => "TXT_BACKGROUND_COLOR",
        "task_bar_text_color" => "TXT_TEXT_COLOR",
        "specify_each_item_color" => "TXT_SPECIFY_EACH_ITEM_COLOR_CHECK",
        "font_name" => "TXT_FONT",
        "font_size" => "TXT_FONT_SIZE",
        "task_bar_wnd_snap" => "TXT_TASKBAR_WINDOWS_CLOSE_TO_ICON",
        "window_offset_top" => "TXT_WINDOW_VERTICAL_OFFSET",
        "window_offset_left" => "TXT_WINDOW_HORIZONTAL_OFFSET",
        "avoid_overlap_with_widgets" => "TXT_AVOID_OVERLAP_RIGHT_WIDGETS_CHECK",
        "dark_default_style" => "TXT_COLOR_PRESET_IN_DARK_MODE",
        "light_default_style" => "TXT_COLOR_PRESET_IN_LIGHT_MODE",
        "auto_save_taskbar_color_settings_to_preset" => "TXT_AUTO_SAVE_TO_PRESET_CHECK",
        "show_netspeed_figure" => "TXT_SHOW_NET_SPEED_GRAPH",
        "netspeed_figure_max_value" => "TXT_NET_SPEED_GRAPH_MAX_VALUE",
        "status_bar_color" => "TXT_USAGE_GRAPH_COLOR",
        "graph_color_following_system" => "TXT_USAGE_GRAPH_FOLLOW_SYSTEM_CHECK",
        "cm_graph_type" => "TXT_GRAPH_DISPLAY_MODE",
        "enable_colorful_emoji" => "TXT_ENABLE_COLOR_EMOJI",
        "use_log_scale" => "TXT_USE_LOG_SCALE",
        "sunday_first" => "TXT_FIRST_DAT_OF_WEEEK",
        "view_type" => "TXT_VIEW_TYPE",
        _ => "",
    }
}
fn localized_option(language: &Language, key: &str, value: &str, existing: &str) -> String {
    let (resource, fallback) = match (key, value) {
        ("language", "") => ("RUST_FOLLOW_SYSTEM", "Follow system"),
        ("skin_selected" | "skin_name_dark_mode" | "skin_name_light_mode", "") => {
            ("RUST_DEFAULT_SKIN", "Default skin")
        }
        ("cpu_usage_acquire_method", "0") => ("TXT_USE_CPU_TIME_RADIO", "Based on CPU time"),
        ("cpu_usage_acquire_method", "1") => ("TXT_USE_PDH_RADIO", "Performance counter"),
        ("memory_display", "0") => ("IDS_MEMORY_USAGE", "Memory usage"),
        ("memory_display", "1") => ("IDS_MEMORY_USED", "Memory used"),
        ("memory_display", "2") => ("IDS_MEMORY_AVAILABLE", "Memory available"),
        ("speed_unit" | "task_bar_speed_unit", "0") => ("IDS_AUTO", "Automatic"),
        ("speed_unit" | "task_bar_speed_unit", "1") => ("", "KB/s"),
        ("speed_unit" | "task_bar_speed_unit", "2") => ("", "MB/s"),
        ("double_click_action", "0") => ("TXT_CONNECTION_DETAILS", "Connection details"),
        ("double_click_action", "1") => ("TXT_HISTORY_TRAFFIC_STATISTICS", "History traffic"),
        ("double_click_action", "2") => ("TXT_SHOW_MORE_INFO", "Show more information"),
        ("double_click_action", "3") => ("TXT_OPTIONS", "Settings"),
        ("double_click_action", "4") => ("TXT_TASK_MANAGER", "Task Manager"),
        ("double_click_action", "5") => ("TXT_EXE_PATH_STATIC", "Specified application"),
        ("double_click_action", "6") => ("TXT_CHANGE_SKIN", "Change skin"),
        ("double_click_action", "7") => ("IDS_NONE", "No action"),
        ("view_type", "0") => ("IDS_DAY_VIEW", "Day"),
        ("view_type", "1") => ("IDS_WEEK_VIEW", "Week"),
        ("view_type", "2") => ("IDS_MONTH_VIEW", "Month"),
        ("view_type", "3") => ("IDS_QUARTER_VIEW", "Quarter"),
        ("view_type", "4") => ("IDS_YEAR_VIEW", "Year"),
        _ => return existing.into(),
    };
    pack_label(language, resource, fallback)
}
/// Owned labels let callers switch language without changing legacy SettingSpec's ABI.
pub fn settings_schema_localized(
    catalog: &LanguageCatalog,
    skins: &[crate::SkinEntry],
    language: &Language,
) -> Vec<LocalizedSettingSpec> {
    settings_schema_with_resources(catalog, skins)
        .into_iter()
        .map(|source| {
            let fallback = english_label(source.key);
            let mut label = pack_label(language, label_key(source.key), &fallback);
            if let Some(item) = DISPLAY_ITEMS.iter().find(|i| i.ini_key == source.key) {
                label = format!(
                    "{} {}",
                    pack_label(language, item.language_key, item.fallback_label),
                    pack_label(language, "RUST_DISPLAY_LABEL", "label")
                );
            }
            if source.section == "task_bar" {
                label = format!(
                    "{}: {label}",
                    pack_label(language, "RUST_TASKBAR", "Taskbar")
                );
            }
            let options = source
                .options
                .iter()
                .map(|o| SettingOption {
                    value: o.value.clone(),
                    label: localized_option(language, source.key, &o.value, &o.label),
                })
                .collect();
            LocalizedSettingSpec {
                section: source.section,
                key: source.key,
                label,
                kind: source.kind,
                default: source.default,
                options,
                min: source.min,
                max: source.max,
                source,
                validation_message: language.text("text", "RUST_INVALID_SETTING", "Invalid value"),
            }
        })
        .collect()
}
impl SettingSpec {
    pub fn value(&self, ini: &IniDocument) -> String {
        ini.get_string(self.section, self.key, self.default)
    }
    pub fn validate(&self, value: &str) -> Result<String, String> {
        if matches!(
            self.kind,
            SettingKind::Text | SettingKind::Font | SettingKind::List
        ) {
            return Ok(value.into());
        }
        let value = value.trim();
        match self.kind {
            SettingKind::Boolean => match value {
                "true" | "1" => Ok("true".into()),
                "false" | "0" => Ok("false".into()),
                _ => Err(format!("{}：请选择开启或关闭", self.label)),
            },
            SettingKind::Integer | SettingKind::Color => {
                let number = value
                    .parse::<i64>()
                    .map_err(|_| format!("{}：请输入整数", self.label))?;
                if self.min.is_some_and(|min| number < min)
                    || self.max.is_some_and(|max| number > max)
                {
                    return Err(format!("{}：数值不在允许范围内", self.label));
                }
                Ok(number.to_string())
            }
            SettingKind::Choice => {
                if self.options.iter().any(|o| o.value == value) {
                    Ok(value.into())
                } else {
                    Err(format!("{}：请选择有效选项", self.label))
                }
            }
            _ => Ok(value.into()),
        }
    }
}
/// Human-facing fields retain the exact legacy keys, including historic misspellings.
pub fn settings_schema() -> Vec<SettingSpec> {
    let mut specs = SCHEMA
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let p: Vec<_> = line.split('|').collect();
            let kind = match p[4] {
                "b" => SettingKind::Boolean,
                "i" => SettingKind::Integer,
                "c" => SettingKind::Choice,
                "color" => SettingKind::Color,
                "font" => SettingKind::Font,
                "list" => SettingKind::List,
                _ => SettingKind::Text,
            };
            let options = p
                .get(7)
                .map(|v| {
                    v.split(';')
                        .filter_map(|v| v.split_once(':'))
                        .map(|(value, label)| SettingOption {
                            value: value.into(),
                            label: label.into(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            SettingSpec {
                section: p[0],
                key: p[1],
                label: p[2],
                default: p[3],
                kind,
                min: p.get(5).and_then(|s| s.parse().ok()),
                max: p.get(6).and_then(|s| s.parse().ok()),
                options,
            }
        })
        .collect::<Vec<_>>();
    for item in DISPLAY_ITEMS {
        specs.push(SettingSpec {
            section: "task_bar",
            key: item.ini_key,
            label: item.fallback_label,
            kind: SettingKind::Text,
            default: "",
            options: Vec::new(),
            min: None,
            max: None,
        });
    }
    specs
}
pub fn settings_schema_with_resources(
    catalog: &LanguageCatalog,
    skins: &[crate::SkinEntry],
) -> Vec<SettingSpec> {
    let mut schema = settings_schema();
    for field in &mut schema {
        if field.section == "general" && field.key == "language" {
            field.kind = SettingKind::Choice;
            field.options = vec![SettingOption {
                value: String::new(),
                label: "跟随系统".into(),
            }];
            field
                .options
                .extend(catalog.languages.iter().map(|l| SettingOption {
                    value: l.config_string(),
                    label: l.display_name.clone(),
                }));
        }
        if field.key == "skin_selected" || field.key.starts_with("skin_name_") {
            field.kind = SettingKind::Choice;
            field.options = vec![SettingOption {
                value: String::new(),
                label: "默认皮肤".into(),
            }];
            field.options.extend(skins.iter().map(|s| SettingOption {
                value: s.name.clone(),
                label: s.name.clone(),
            }));
        }
    }
    schema
}
pub fn apply_setting(
    ini: &mut IniDocument,
    section: &str,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let spec = settings_schema()
        .into_iter()
        .find(|s| s.section == section && s.key == key)
        .ok_or_else(|| format!("未知设置：{section}/{key}"))?;
    let value = spec.validate(value)?;
    ini.set(section, key, value);
    Ok(())
}
/// Materialize defaults and correct invalid known numbers, preserving vendor sections.
pub fn normalize_settings(ini: &mut IniDocument, language: &str, light_theme: bool) -> Vec<String> {
    let mut warnings = Vec::new();
    for spec in settings_schema() {
        // Missing labels are derived from the selected language by the renderer.
        // An explicitly saved empty label must remain empty.
        if spec.section == "task_bar"
            && DISPLAY_ITEMS.iter().any(|i| i.ini_key == spec.key)
            && ini.get(spec.section, spec.key).is_none()
        {
            continue;
        }
        if let Some(value) = ini.get(spec.section, spec.key) {
            if spec.validate(value).is_err() {
                warnings.push(format!(
                    "{}/{}: invalid value replaced by default",
                    spec.section, spec.key
                ));
                ini.set(spec.section, spec.key, spec.default);
            }
        } else {
            let default = match (spec.section, spec.key) {
                ("general", "update_source") if language.starts_with("zh-CN") => "1",
                ("other", "last_light_mode") if light_theme => "true",
                _ => spec.default,
            };
            ini.set(spec.section, spec.key, default);
        }
    }
    warnings
}
const SCHEMA: &str = r#"general|check_update_when_start|启动时检查更新|true|b
general|show_all_interface|显示所有网络接口|false|b
general|language|界面语言||t
general|update_source|更新来源|0|c|||0:GitHub;1:Gitee
general|cpu_usage_acquire_method|CPU 使用率获取方式|1|c|||0:系统时间;1:性能计数器
general|monitor_time_span|采样间隔（毫秒）|1000|i|200|30000
general|hard_disk_name|监控硬盘||t
general|cpu_core_name|CPU 核心|Core Average|t
general|hardware_monitor_item|硬件监控项目|0|i|0|15
general|connections_hide|隐藏的网络连接||list
config|transparency|悬浮窗不透明度（百分比）|80|i|0|100
config|always_on_top|悬浮窗置顶|true|b
config|lock_window_pos|锁定悬浮窗位置|false|b
config|show_notify_icon|显示通知区域图标|true|b
config|show_cpu_memory|显示更多信息|false|b
config|mouse_penetrate|鼠标穿透|false|b
config|show_task_bar_wnd|显示任务栏窗口|false|b
config|position_x|悬浮窗横坐标|-1|i|-2147483648|2147483647
config|position_y|悬浮窗纵坐标|-1|i|-2147483648|2147483647
config|hide_main_window|隐藏悬浮窗|false|b
config|skin_selected|悬浮窗皮肤||t
config|notify_icon_selected|通知图标样式|0|i|0|31
config|notify_icon_auto_adapt|通知图标跟随系统主题|true|b
config|swap_up_down|交换上传和下载|false|b
config|hide_main_wnd_when_fullscreen|全屏时隐藏悬浮窗|true|b
config|speed_short_mode|网速简洁显示|false|b
config|separate_value_unit_with_space|数值和单位间添加空格|true|b
config|show_tool_tip|显示悬浮提示|true|b
config|memory_display|内存显示方式|0|c|||0:使用率;1:已使用;2:可用
config|unit_byte|以字节显示流量|true|b
config|speed_unit|网速单位|0|c|||0:自动;1:KB/s;2:MB/s
config|hide_unit|隐藏网速单位|false|b
config|hide_percent|隐藏百分号|false|b
config|double_click_action|悬浮窗双击操作|0|c|||0:连接详情;1:历史流量;2:显示更多信息;3:选项设置;4:任务管理器;5:指定应用;6:更换皮肤;7:无操作
config|double_click_exe|双击打开的应用||t
config|alow_out_of_border|允许悬浮窗超出屏幕边界|false|b
config|plugin_disabled|禁用的插件||list
connection|auto_select|自动选择网络连接|true|b
connection|select_all|统计所有网络连接|false|b
connection|connection_name|指定网络连接||t
skins|skin_auto_adapt|皮肤跟随系统主题|false|b
skins|skin_name_dark_mode|深色主题皮肤||t
skins|skin_name_light_mode|浅色主题皮肤||t
notify_tip|traffic_tip_enable|启用流量提醒|false|b
notify_tip|traffic_tip_value|流量提醒阈值|200|i|1|2147483647
notify_tip|traffic_tip_unit|流量提醒单位|0|c|||0:MB;1:GB
notify_tip|memory_usage_tip_enable|启用内存使用率提醒|false|b
notify_tip|memory_tip_value|内存使用率提醒阈值|80|i|1|100
notify_tip|cpu_temperature_tip_enable|启用 CPU 温度提醒|false|b
notify_tip|cpu_temperature_tip_value|CPU 温度提醒阈值|80|i|1|200
notify_tip|gpu_temperature_tip_enable|启用 GPU 温度提醒|false|b
notify_tip|gpu_temperature_tip_value|GPU 温度提醒阈值|80|i|1|200
notify_tip|hdd_temperature_tip_enable|启用硬盘温度提醒|false|b
notify_tip|hdd_temperature_tip_value|硬盘温度提醒阈值|80|i|1|200
notify_tip|mainboard_temperature_tip_enable|启用主板温度提醒|false|b
notify_tip|mainboard_temperature_tip_value|主板温度提醒阈值|80|i|1|200
task_bar|task_bar_back_color|任务栏背景颜色|0|color|0|16777215
task_bar|transparent_color|任务栏透明颜色|0|color|0|16777215
task_bar|status_bar_color|状态图颜色|5921370|color|0|16777215
task_bar|task_bar_text_color|项目文字颜色|16777215|list
task_bar|specify_each_item_color|分别指定各项目颜色|false|b
task_bar|tbar_display_item|任务栏显示项目|15|i|0|8191
task_bar|font_name|任务栏字体|Segoe UI|font
task_bar|font_size|任务栏字号|9|i|5|72
task_bar|font_style|任务栏字体样式|0|i|0|15
task_bar|show_taskbar_wnd_in_secondary_display|在副显示器上显示|false|b
task_bar|secondary_display_index|副显示器编号|0|i|0|64
task_bar|task_bar_wnd_on_left|任务栏左侧显示|false|b
task_bar|task_bar_wnd_snap|吸附任务栏边缘|false|b
task_bar|task_bar_speed_short_mode|任务栏网速简洁显示|true|b
task_bar|unit_byte|任务栏以字节显示|true|b
task_bar|task_bar_speed_unit|任务栏网速单位|0|c|||0:自动;1:KB/s;2:MB/s
task_bar|task_bar_hide_unit|任务栏隐藏单位|false|b
task_bar|task_bar_hide_percent|任务栏隐藏百分号|false|b
task_bar|value_right_align|数值右对齐|true|b
task_bar|horizontal_arrange|水平排列显示项目|false|b
task_bar|show_status_bar|显示状态图|true|b
task_bar|separate_value_unit_with_space|任务栏数值与单位间添加空格|true|b
task_bar|show_tool_tip|显示任务栏提示|true|b
task_bar|digits_number|任务栏数值位数|4|i|3|12
task_bar|memory_display|任务栏内存显示方式|0|c|||0:使用率;1:已使用;2:可用
task_bar|double_click_action|任务栏双击操作|0|c|||0:连接详情;1:历史流量;2:显示更多信息;3:选项设置;4:任务管理器;5:指定应用;6:更换皮肤;7:无操作
task_bar|double_click_exe|任务栏双击打开的应用||t
task_bar|cm_graph_type|使用条状图|true|b
task_bar|show_graph_dashed_box|图表显示虚线边框|false|b
task_bar|item_space|项目间距|8|i|0|32
task_bar|vertical_margin|垂直边距|0|i|-10|10
task_bar|window_offset_top|任务栏窗口垂直偏移|0|i|-20|20
task_bar|window_offset_left|任务栏窗口水平偏移|0|i|-800|800
task_bar|avoid_overlap_with_widgets|避让小组件|false|b
task_bar|taskbar_left_space_win11|Windows 11 左侧保留空间|160|i|0|4096
task_bar|taskbar_right_space_win11|Windows 11 右侧保留空间|280|i|0|4096
task_bar|auto_adapt_light_theme|任务栏跟随系统主题|false|b
task_bar|dark_default_style|深色主题预设|0|i|0|15
task_bar|light_default_style|浅色主题预设|3|i|-1|15
task_bar|auto_set_background_color|自动设置背景色|false|b
task_bar|item_order|任务栏项目顺序||list
task_bar|plugin_display_item|任务栏插件项目||list
task_bar|auto_save_taskbar_color_settings_to_preset|颜色修改保存到主题预设|true|b
task_bar|show_netspeed_figure|显示网速图表|true|b
task_bar|netspeed_figure_max_value|网速图表量程|10|i|1|2147483647
task_bar|netspeed_figure_max_value_unit|网速图表量程单位|1|c|||0:KB/s;1:MB/s
task_bar|graph_color_following_system|图表颜色跟随系统|true|b
task_bar|disable_d2d|使用 GDI 绘图|true|b
task_bar|enable_colorful_emoji|显示彩色表情|true|b
histroy_traffic|use_log_scale|历史流量使用对数比例|true|b
histroy_traffic|sunday_first|每周从周日开始|true|b
histroy_traffic|view_type|历史流量查看范围|0|c|||0:日;1:周;2:月;3:季度;4:年
other|notify_interval|提醒间隔（秒）|60|i|1|86400
other|no_multistart_warning|允许多开时不显示警告|false|b
other|exit_when_start_by_restart_manager|系统自动恢复时退出|true|b
other|debug_log|记录诊断日志|false|b
other|auto_run_use_task_scheduler|使用任务计划自动启动|false|b
other|taksbar_transparent_color_enable|允许任务栏透明色|true|b
other|last_light_mode|上次为浅色主题|false|b
other|show_mouse_panetrate_tip|显示鼠标穿透提示|true|b
other|show_dot_net_notinstalled_tip|显示硬件监控组件提示|true|b"#;
