//! Existing resource keys are reused; only genuinely new frontend wording has built-in fallback.
pub(crate) fn legacy_key(key: &str) -> &str {
    match key {
        "settings" => "menu/TXT_OPTIONS",
        "history" => "menu/TXT_HISTORY_TRAFFIC_STATISTICS",
        "network" => "menu/TXT_SELECT_CONNECTIONS",
        "appearance" => "menu/TXT_CHANGE_SKIN",
        "plugins" => "menu/TXT_PLUGIN_MANAGE",
        "show_main" => "menu/TXT_SHOW_MAIN_WINDOW",
        "show_taskbar" => "menu/TXT_SHOW_TASKBAR_WINDOW",
        "topmost" => "text/TXT_ALWAYS_ON_TOP_CHECK",
        "lock" | "lock_position" => "text/TXT_LOCK_WINDOW_POS_CHECK",
        "click_through" => "text/TXT_MOUSE_PENETRATE_CHECK",
        "update" => "menu/TXT_CHECK_UPDATE",
        "exit" => "menu/TXT_EXIT",
        "data" => "text/TXT_OPEN_CONFIG_PATH_BUTTON",
        "taskbar_settings" => "text/IDS_TASKBAR_WINDOW_SETTINGS",
        "Settings" => "text/TXT_TITLE_OPTION",
        "Taskbar appearance" => "text/IDS_TASKBAR_WINDOW_SETTINGS",
        "Traffic history" => "text/TXT_TITLE_HISTORY_TRAFFIC",
        "Network connections" => "text/TXT_TITLE_SELECTION_CONNECTION",
        "Plugin manager" => "text/TXT_TITLE_PLUGIN_MANAGE",
        "Apply" => "text/TXT_APPLY",
        "Cancel" => "text/TXT_CANCEL",
        "OK" => "text/TXT_OK",
        "Advanced…" | "Advanced settings" => "text/TXT_ADVANCED",
        "General…" => "text/IDS_GENERAL_SETTINGS",
        "show_tray" => "text/TXT_SHOW_NOTIFY_ICON_CHECK",
        "hide_fullscreen" => "text/TXT_FULLSCREEN_HIDE_CHECK",
        "font_name" | "Font family" => "text/TXT_FONT",
        "font_size" | "Font size" => "text/TXT_FONT_SIZE",
        "foreground" | "Text color (#RRGGBB)" => "text/TXT_TEXT_COLOR",
        "background" | "Background (#RRGGBB)" => "text/TXT_BACKGROUND_COLOR",
        "item_gap" => "text/TXT_ITEM_SPACING",
        "interval_ms" => "text/TXT_MONITORING_INTERVALS",
        "Double click action" => "text/TXT_DOUBLE_CLICK_ACTION",
        "Show tooltip" => "text/TXT_SHOW_TOOL_TIP_CHK",
        "Swap upload / download" => "text/TXT_SWITCH_UP_DOWN_CHECK",
        "Right align values" => "text/TXT_VALUE_RIGHT_ALIGN",
        "Show history graphs" => "text/TXT_SHOW_NET_SPEED_GRAPH",
        "Show status bars" => "text/TXT_SHOW_RESOURCE_USAGE_GRAPH",
        "Place at left / top" => "text/TXT_TASKBAR_WND_ON_LEFT",
        "Vertical offset" => "text/TXT_WINDOW_VERTICAL_OFFSET",
        "Vertical margin" => "text/TXT_VERTICAL_MARGIN",
        "Graph style" => "text/TXT_GRAPH_DISPLAY_MODE",
        "Bar" => "text/TXT_BAR_MODE",
        "Line" => "text/TXT_PLOT_MODE",
        "Show dashed frame" => "text/TXT_SHOW_DASHED_BOX",
        "Auto background color" => "text/TXT_AUTO_SET_BACK_COLOR",
        "Show more information" => "text/IDS_SHOW_HIDE_MORE_INFO",
        "Network" => "text/IDS_OPEN_CONNECTION_DETIAL",
        "History" => "text/IDS_OPEN_HISTORICAL_TRAFFIC",
        "Task Manager" => "menu/TXT_TASK_MANAGER",
        "Next skin" => "text/IDS_CHANGE_SKIN",
        "Language" => "text/TXT_LANGUAGE",
        "Skin" => "text/TXT_SELECT_A_SKIN",
        "Date / period" => "text/IDS_DATE",
        "Upload" => "text/IDS_UPLOAD",
        "Download" => "text/IDS_DOWNLOAD",
        "Day" => "text/IDS_DAY_VIEW",
        "Week" => "text/IDS_WEEK_VIEW",
        "Month" => "text/IDS_MONTH_VIEW",
        "Quarter" => "text/IDS_QUARTER_VIEW",
        "Year" => "text/IDS_YEAR_VIEW",
        "Logarithmic chart" => "menu/TXT_USE_LOG_SCALE",
        "Configure…" => "text/TXT_OPTINS_BUTTON",
        _ => key,
    }
}
pub(crate) fn fallback<'a>(key: &str, english: &'a str, locale: &str) -> &'a str {
    if !locale.to_ascii_lowercase().starts_with("zh") {
        return english;
    }
    match key {
        "General" => "常规",
        "Floating window" => "悬浮窗",
        "Taskbar" => "任务栏",
        "Display items" => "显示项目",
        "Alerts" => "提醒",
        "Automatically fit content width" => "自动适应内容宽度",
        "Minimum taskbar width" => "任务栏最小宽度",
        "Recommended items" => "推荐项目",
        "Move up" => "上移",
        "Move down" => "下移",
        "Unavailable" => "暂不可用",
        "Upload (B/s)" => "上传速度（B/s）",
        "Download (B/s)" => "下载速度（B/s）",
        "CPU usage (%)" => "CPU 使用率（%）",
        "Memory (%)" => "内存使用情况（%）",
        "GPU usage (%)" => "GPU 使用率（%）",
        "CPU temperature (°C)" => "CPU 温度（°C）",
        "GPU temperature (°C)" => "GPU 温度（°C）",
        "Disk temperature (°C)" => "硬盘温度（°C）",
        "Motherboard temperature (°C)" => "主板温度（°C）",
        "Disk activity (%)" => "硬盘使用率（%）",
        "Total speed (B/s)" => "总网速（B/s）",
        "CPU frequency (GHz)" => "CPU 频率（GHz）",
        "Today traffic (B)" => "今日流量（B）",
        "Floating display items are defined by the selected skin." => {
            "悬浮窗显示项目由当前皮肤定义。"
        }
        "Check items to display; select a row and use Move up / Move down to change order." => {
            "勾选需要显示的项目；选中一行后用“上移 / 下移”调整顺序。"
        }
        "Select at least one floating display item" => "请至少选择一个悬浮窗显示项目",
        "snap_to_edges" => "吸附屏幕边缘",
        "opacity" | "Opacity (0–255)" => "不透明度（0–255）",
        "width" => "窗口宽度",
        "height" => "窗口高度",
        "taskbar_width" => "任务栏最小宽度",
        "taskbar_offset" => "任务栏水平偏移",
        "taskbar_monitor" => "显示器（0为主屏）",
        "taskbar_two_rows" => "任务栏双行显示",
        "dark" => "深色主题",
        "follow_system_theme" => "跟随系统主题",
        "visible_items" | "Visible IDs in order" => "按顺序显示的项目ID",
        "Skins and language" => "皮肤与语言",
        "Settings…" => "设置…",
        "History…" => "历史流量…",
        "Network connections…" => "网络连接…",
        "Skins and language…" => "皮肤与语言…",
        "Plugins…" => "插件管理…",
        "settings" => "设置…",
        "history" => "历史流量…",
        "network" => "网络连接…",
        "appearance" => "皮肤与语言…",
        "plugins" => "插件管理…",
        "show_main" => "显示悬浮窗口",
        "show_taskbar" => "显示任务栏窗口",
        "topmost" => "总在最前",
        "lock" | "lock_position" => "锁定窗口位置",
        "click_through" => "鼠标穿透",
        "update" => "检查更新",
        "exit" => "退出",
        "data" => "打开数据目录",
        "taskbar_settings" => "任务栏外观…",
        "Settings" => "设置",
        "Taskbar appearance" => "任务栏窗口设置",
        "Traffic history" => "历史流量统计",
        "Network connections" => "选择监控的网络连接",
        "Plugin manager" => "插件管理",
        "Advanced settings" | "Advanced…" => "高级设置",
        "General…" => "常规设置",
        "Apply" => "应用",
        "Cancel" => "取消",
        "OK" => "确定",
        "Bold" => "粗体",
        "Italic" => "斜体",
        "Underline" => "下划线",
        "Strikeout" => "删除线",
        "Label color (blank = text)" => "标签颜色（空为默认）",
        "Value color (blank = text)" => "数值颜色（空为默认）",
        "Graph max (blank = auto)" => "图表上限（空为自动）",
        "Graph color (blank = text)" => "图表颜色（空为默认）",
        "Transparent color (optional)" => "透明色（可选）",
        "Show labels" => "显示标签",
        "From (YYYY-MM-DD)" => "起始日期（年-月-日）",
        "To" => "截止日期",
        "Apply range" => "应用日期范围",
        "Import…" => "导入…",
        "Export…" => "导出…",
        "Mixed / unknown" => "方向未知流量",
        "Total" => "合计",
        "Click a date to select a single day." => "点击日历日期查看当天流量。",
        "Select one or more adapters. Automatic mode avoids duplicate virtual/filter interfaces." => {
            "选择一个或多个网卡；自动模式避免重复统计虚拟及过滤接口。"
        }
        "Select an adapter to inspect addresses, counters and status." => {
            "选择网卡查看地址、计数器和状态。"
        }
        "Automatic active adapter" => "自动选择活动网卡",
        "All eligible adapters" => "全部有效网卡",
        "Selected adapters" => "手动选择的网卡",
        "Apply connection selection" => "应用连接选择",
        "Apply skin and language" => "应用皮肤与语言",
        "Apply a skin to preview it in the live floating monitor. Skin item positions, colors and PNG/BMP backgrounds are rendered by the desktop frontend." => {
            "应用后可在悬浮窗口实时预览皮肤。皮肤支持独立项目位置、颜色和透明背景。"
        }
        "Item IDs and order: leave empty to show all. Available IDs:" => {
            "显示项目ID与顺序：留空显示全部，可选项目如下："
        }
        "Double click action" => "双击动作",
        "Toggle floating monitor" => "切换悬浮窗口显示",
        "Open configured application" => "打开指定应用",
        "Do nothing" => "不执行操作",
        "Auto start (Task Scheduler)" => "开机启动（任务计划程序）",
        "Auto start (Registry)" => "开机启动（注册表）",
        "Auto start status unavailable" => "无法读取开机启动状态",
        "Current progress" => "当前数值进度条",
        "History bars" => "历史柱状图",
        "Enable / disable" => "启用 / 禁用",
        "Run selected command" => "执行所选命令",
        "edit_list" => "编辑显示项目",
        "Add item" => "添加项目",
        "Remove selected" => "删除选中项",
        "Window size, background and display items are defined by the current skin. Available IDs:" => {
            "窗口大小、背景与显示项目由当前皮肤定义。可用项目ID如下："
        }
        _ => english,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tab_and_checkbox_wording_is_available_in_chinese() {
        assert_eq!(fallback("General", "General", "zh-CN"), "常规");
        assert_eq!(
            fallback("Display items", "Display items", "zh-CN"),
            "显示项目"
        );
        assert_eq!(
            fallback("CPU temperature (°C)", "CPU temperature (°C)", "zh-CN"),
            "CPU 温度（°C）"
        );
        assert_eq!(
            fallback(
                "Automatically fit content width",
                "Automatically fit content width",
                "zh-CN"
            ),
            "自动适应内容宽度"
        );
    }
    #[test]
    fn reuses_resource_keys_for_menus_and_titles() {
        assert_eq!(legacy_key("settings"), "menu/TXT_OPTIONS");
        assert_eq!(
            legacy_key("Traffic history"),
            "text/TXT_TITLE_HISTORY_TRAFFIC"
        );
    }
    #[test]
    fn new_wording_has_explicit_locale_fallback() {
        assert_eq!(
            fallback("Apply range", "Apply range", "zh-CN"),
            "应用日期范围"
        );
        assert_eq!(
            fallback("Apply range", "Apply range", "en-US"),
            "Apply range"
        );
        assert_eq!(
            fallback("Apply range", "Apply range", "de-DE"),
            "Apply range"
        );
    }
}
