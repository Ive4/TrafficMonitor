//! Persisted legacy settings mapped into the live desktop contract.
use std::path::Path;
use traffic_monitor_config::{
    Alignment, DISPLAY_ITEMS, FontInfo, IniDocument, LanguageCatalog, Skin, display_order,
};
use traffic_monitor_desktop_ui::{
    DesktopOptions, DoubleClickAction, GraphStyle, ItemColors, SkinItem, SkinLayout, TaskbarStyle,
    TextAlignment,
};
fn int(ini: &IniDocument, s: &str, k: &str, d: i64, min: i64, max: i64) -> i64 {
    ini.get_i64(s, k, d).clamp(min, max)
}
fn color(ini: &IniDocument, s: &str, k: &str, d: u32) -> u32 {
    int(ini, s, k, d.into(), 0, 0xffffff) as u32
}
fn themed_graph_color(color: u32, light: bool) -> u32 {
    let r = f64::from(color & 255) / 255.0;
    let g = f64::from((color >> 8) & 255) / 255.0;
    let b = f64::from((color >> 16) & 255) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let saturation = if delta == 0.0 {
        0.0
    } else {
        delta / (1.0 - (max + min - 1.0).abs())
    };
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    let lightness: f64 = if light { 0.7 } else { 0.4 };
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hue as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    let channel = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    channel(r) | (channel(g) << 8) | (channel(b) << 16)
}
fn graph_color(ini: &IniDocument, dark: bool) -> u32 {
    let configured = color(ini, "task_bar", "status_bar_color", 0x5a5a5a);
    if ini.get_bool("task_bar", "graph_color_following_system", true) {
        #[cfg(windows)]
        if let Ok(theme) = traffic_monitor_windows::services::current_theme() {
            return themed_graph_color(theme.color, !dark);
        }
    }
    configured
}
fn action(v: i64) -> DoubleClickAction {
    match v {
        0 => DoubleClickAction::Network,
        1 => DoubleClickAction::History,
        2 => DoubleClickAction::ToggleMore,
        3 => DoubleClickAction::Settings,
        4 => DoubleClickAction::TaskManager,
        5 => DoubleClickAction::SpecificApp,
        6 => DoubleClickAction::ChangeSkin,
        _ => DoubleClickAction::None,
    }
}
fn action_value(v: DoubleClickAction) -> i64 {
    match v {
        DoubleClickAction::Network => 0,
        DoubleClickAction::History => 1,
        DoubleClickAction::ToggleMore => 2,
        DoubleClickAction::Settings => 3,
        DoubleClickAction::TaskManager => 4,
        DoubleClickAction::SpecificApp => 5,
        DoubleClickAction::ChangeSkin => 6,
        DoubleClickAction::None | DoubleClickAction::ToggleMain => 7,
    }
}
fn font(ini: &IniDocument, s: &str, default: &FontInfo) -> FontInfo {
    let mut f = FontInfo::load(ini, s, default);
    if f.name.is_empty() {
        f.name = default.name.clone();
    }
    if !(5..=72).contains(&f.size) {
        f.size = default.size;
    }
    f.style &= 15;
    f
}
fn colors(ini: &IniDocument) -> Vec<(u32, u32)> {
    let v: Vec<_> = ini
        .get("task_bar", "task_bar_text_color")
        .unwrap_or("")
        .split(',')
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().parse::<u32>().unwrap_or(0).min(0xffffff))
        .collect();
    (0..13)
        .map(|i| {
            (
                v.get(i * 2).copied().unwrap_or(0xffffff),
                v.get(i * 2 + 1).copied().unwrap_or(0xffffff),
            )
        })
        .collect()
}
fn ordered_items(ini: &IniDocument) -> Vec<String> {
    let mask = ini.get_i64("task_bar", "tbar_display_item", 15) as u64;
    let mut ids = display_order(&ini.get_string("task_bar", "item_order", ""))
        .into_iter()
        .filter(|id| {
            DISPLAY_ITEMS
                .iter()
                .any(|item| item.id == *id && mask & (1u64 << item.index) != 0)
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for id in ini
        .get_string("task_bar", "plugin_display_item", "")
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        if !ids.iter().any(|v| v == id) {
            ids.push(id.into());
        }
    }
    if ini.get("rust_taskbar", "item_order").is_some() {
        let mut ordered = Vec::new();
        for id in ini.get_string_list("rust_taskbar", "item_order") {
            if ids.contains(&id) && !ordered.contains(&id) {
                ordered.push(id);
            }
        }
        for id in ids {
            if !ordered.contains(&id) {
                ordered.push(id);
            }
        }
        ordered
    } else {
        ids
    }
}

fn skin_order(skin: &Skin, plugins: &[String]) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    for (id, item) in skin.small.items.iter().chain(skin.large.items.iter()) {
        if DISPLAY_ITEMS.iter().any(|i| i.id == id) {
            ids.insert(id.clone());
        } else if let Some(plugin) = &item.plugin_id {
            if plugins.is_empty() || plugins.contains(plugin) {
                ids.insert(plugin.clone());
            }
        }
    }
    let mut ids: Vec<_> = ids.into_iter().collect();
    ids.sort_by_key(|id| {
        DISPLAY_ITEMS
            .iter()
            .find(|i| i.id == id)
            .map(|i| usize::from(i.index))
            .unwrap_or_else(|| {
                13 + plugins
                    .iter()
                    .position(|p| p == id)
                    .unwrap_or(plugins.len())
            })
    });
    ids
}
fn preset_index(ini: &IniDocument, dark: bool) -> Option<i64> {
    if !ini.get_bool("task_bar", "auto_adapt_light_theme", false) {
        return None;
    }
    let value = ini.get_i64(
        "task_bar",
        if dark {
            "dark_default_style"
        } else {
            "light_default_style"
        },
        if dark { 0 } else { 3 },
    );
    (0..4).contains(&value).then_some(value)
}
fn theme_preset(ini: &IniDocument, dark: bool) -> IniDocument {
    let mut effective = ini.clone();
    let Some(index) = preset_index(ini, dark) else {
        return effective;
    };
    let prefix = format!("default{}_", index + 1);
    let light = matches!(index, 2 | 3);
    let fallback = match index {
        1 => 4574711,
        2 => 31957,
        _ => {
            if light {
                0
            } else {
                0xffffff
            }
        }
    };
    let defaults = match index {
        1 => {
            "4574711,16777215,5107653,16777215,16632499,16777215,16494036,16777215,14601983,16777215,16760992,10221779,13425662,16777215,4574711,16777215"
        }
        2 => "31957,0,2533120,0,13857281,0,14354551,0,1145775,0,4328380,0,167504,0,31957,0",
        _ => "",
    };
    let raw = ini.get_string(
        "taskbar_default_style",
        &format!("{prefix}text_color"),
        defaults,
    );
    let mut colors = raw
        .split(',')
        .filter(|v| !v.is_empty())
        .filter_map(|v| v.trim().parse::<u32>().ok())
        .collect::<Vec<_>>();
    colors.resize(colors.len().max(26), fallback);
    let back = color(
        ini,
        "taskbar_default_style",
        &format!("{prefix}back_color"),
        if light { 0xd3d2d2 } else { 0 },
    );
    if colors.iter().all(|color| *color == back) {
        return effective;
    }
    effective.set(
        "task_bar",
        "task_bar_text_color",
        colors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
    );
    effective.set_i64("task_bar", "task_bar_back_color", back.into());
    effective.set_i64(
        "task_bar",
        "transparent_color",
        color(
            ini,
            "taskbar_default_style",
            &format!("{prefix}transparent_color"),
            if light { 0xd3d2d2 } else { 0 },
        )
        .into(),
    );
    effective.set_i64(
        "task_bar",
        "status_bar_color",
        color(
            ini,
            "taskbar_default_style",
            &format!("{prefix}status_bar_color"),
            if light { 0xa5a5a5 } else { 0x5a5a5a },
        )
        .into(),
    );
    effective.set_bool(
        "task_bar",
        "specify_each_item_color",
        ini.get_bool(
            "taskbar_default_style",
            &format!("{prefix}specify_each_item_color"),
            matches!(index, 1 | 2),
        ),
    );
    effective
}
fn save_theme_preset(ini: &mut IniDocument, dark: bool) {
    if !ini.get_bool(
        "task_bar",
        "auto_save_taskbar_color_settings_to_preset",
        true,
    ) {
        return;
    }
    let Some(index) = preset_index(ini, dark) else {
        return;
    };
    let prefix = format!("default{}_", index + 1);
    let back = ini.get_i64("task_bar", "task_bar_back_color", 0);
    let raw = ini.get_string("task_bar", "task_bar_text_color", "");
    if raw
        .split(',')
        .filter(|v| !v.is_empty())
        .all(|v| v.parse::<i64>().ok() == Some(back))
    {
        return;
    }
    for (from, to) in [
        ("task_bar_text_color", "text_color"),
        ("task_bar_back_color", "back_color"),
        ("transparent_color", "transparent_color"),
        ("status_bar_color", "status_bar_color"),
        ("specify_each_item_color", "specify_each_item_color"),
    ] {
        if let Some(value) = ini.get("task_bar", from).map(str::to_owned) {
            ini.set("taskbar_default_style", &format!("{prefix}{to}"), value);
        }
    }
}
pub fn options(
    ini: &IniDocument,
    resources: &Path,
    source: Option<&Skin>,
    dark: bool,
) -> DesktopOptions {
    options_with_plugin_order(ini, resources, source, dark, &[])
}
/// Pass the loaded plugin item sequence to reproduce the original compact skin color order.
pub fn options_with_plugin_order(
    ini: &IniDocument,
    resources: &Path,
    source: Option<&Skin>,
    dark: bool,
    plugin_order: &[String],
) -> DesktopOptions {
    let effective = theme_preset(ini, dark);
    let ini = &effective;
    let language = LanguageCatalog::load(resources.join("language"))
        .ok()
        .map(|c| c.select(&ini.get_string("general", "language", "")));
    let default = FontInfo {
        name: language
            .map(|l| l.info.default_font)
            .unwrap_or_else(|| "Segoe UI".into()),
        size: 10,
        style: 0,
    };
    let main = font(ini, "rust_main", &default);
    let task = font(ini, "task_bar", &FontInfo { size: 9, ..default });
    let mut o = DesktopOptions {
        resource_dir: resources.to_owned(),
        font_name: main.name,
        font_size: main.size,
        font_style: main.style,
        ..DesktopOptions::default()
    };
    o.history_log_scale = ini.get_bool("histroy_traffic", "use_log_scale", true);
    let selected = ini.get_i64("config", "notify_icon_selected", 0);
    let selected = if (0..6).contains(&selected) {
        selected as usize
    } else {
        0
    };
    let selected = if ini.get_bool("config", "notify_icon_auto_adapt", true) {
        match (selected, dark) {
            (0, false) => 4,
            (1, false) => 5,
            (4, true) => 0,
            (5, true) => 1,
            _ => selected,
        }
    } else {
        selected
    };
    let icon = [
        "notifyicon.ico",
        "notifyicon2.ico",
        "notifyicon3.ico",
        "TrafficMonitor.ico",
        "notifyicon4.ico",
        "notifyicon5.ico",
    ][selected];
    o.tray_icon = [resources.join("res").join(icon), resources.join(icon)]
        .into_iter()
        .find(|path| path.is_file());
    o.show_main = !ini.get_bool("config", "hide_main_window", false);
    o.show_taskbar = ini.get_bool("config", "show_task_bar_wnd", false);
    o.show_tray = ini.get_bool("config", "show_notify_icon", true);
    o.topmost = ini.get_bool("config", "always_on_top", true);
    o.click_through = ini.get_bool("config", "mouse_penetrate", false);
    o.lock_position = ini.get_bool("config", "lock_window_pos", false);
    o.snap_to_edges = !ini.get_bool("config", "alow_out_of_border", false);
    o.hide_fullscreen = ini.get_bool("config", "hide_main_wnd_when_fullscreen", true);
    o.tooltip = ini.get_bool("config", "show_tool_tip", true);
    o.swap_up_down = ini.get_bool("config", "swap_up_down", false);
    o.opacity = (int(ini, "config", "transparency", 80, 0, 100) * 255 / 100) as u8;
    o.x = int(
        ini,
        "config",
        "position_x",
        -1,
        i32::MIN.into(),
        i32::MAX.into(),
    ) as i32;
    o.y = int(
        ini,
        "config",
        "position_y",
        -1,
        i32::MIN.into(),
        i32::MAX.into(),
    ) as i32;
    if o.x == -1 && o.y == -1 {
        o.x = 40;
        o.y = 40;
    }
    let interval = ini.get_i64("general", "monitor_time_span", 1000);
    o.interval_ms = if (200..=30000).contains(&interval) {
        interval as u32
    } else {
        1000
    };
    o.dark = dark;
    o.follow_system_theme = ini.get_bool("task_bar", "auto_adapt_light_theme", false)
        || ini.get_bool("skins", "skin_auto_adapt", false);
    o.foreground = color(ini, "rust_main", "foreground", 0xffffff);
    o.background = color(
        ini,
        "rust_main",
        "background",
        if dark { 0x251e18 } else { 0xeeeeee },
    );
    o.visible_items = vec!["up".into(), "down".into()];
    if ini.get_bool("config", "show_cpu_memory", false) {
        o.visible_items.extend(["cpu".into(), "memory".into()]);
    }
    if let Some(v) = ini.get("rust_main", "visible_items") {
        o.visible_items = v
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
    }
    o.taskbar_monitor = if ini.get_bool("task_bar", "show_taskbar_wnd_in_secondary_display", false)
    {
        int(ini, "task_bar", "secondary_display_index", 0, 0, 63) as usize + 1
    } else {
        0
    };
    o.taskbar_width = int(ini, "rust_taskbar", "width", 180, 1, 4096) as i32;
    o.taskbar_offset = int(ini, "task_bar", "window_offset_left", 0, -800, 800) as i32;
    o.taskbar_two_rows = !ini.get_bool("task_bar", "horizontal_arrange", false);
    o.item_gap = int(ini, "task_bar", "item_space", 8, 0, 32) as i32;
    let c = colors(ini);
    let background = color(ini, "task_bar", "task_bar_back_color", 0);
    let transparent = color(ini, "task_bar", "transparent_color", 0);
    let independent = ini.get_bool("task_bar", "specify_each_item_color", false);
    o.taskbar_style = TaskbarStyle {
        auto_width: ini.get_bool(
            "rust_taskbar",
            "auto_width",
            ini.get("rust_taskbar", "width")
                .is_none_or(|w| w.trim() == "180"),
        ),
        value_digits: int(ini, "task_bar", "digits_number", 4, 3, 12) as u8,
        visible_items: ordered_items(ini),
        font_name: task.name,
        font_size: task.size,
        font_style: task.style,
        foreground: c[0].0,
        background,
        opacity: int(ini, "rust_taskbar", "opacity", 255, 0, 255) as u8,
        transparent_color: if ini.get_bool("other", "taksbar_transparent_color_enable", true)
            && (transparent == background || dark && transparent == 0)
        {
            Some(transparent)
        } else {
            None
        },
        top_offset: int(ini, "task_bar", "window_offset_top", 0, -20, 20) as i32,
        vertical_margin: int(ini, "task_bar", "vertical_margin", 0, -10, 10) as i32,
        graph_maximum: Some(
            int(
                ini,
                "task_bar",
                "netspeed_figure_max_value",
                10,
                1,
                i32::MAX.into(),
            ) as f64
                * if ini.get_i64("task_bar", "netspeed_figure_max_value_unit", 1) == 0 {
                    1024.0
                } else {
                    1048576.0
                },
        ),
        graph_color: Some(graph_color(ini, dark)),
        graph_style: if ini.get_bool("task_bar", "cm_graph_type", true) {
            GraphStyle::Bar
        } else {
            GraphStyle::Line
        },
        item_colors: if independent {
            DISPLAY_ITEMS
                .iter()
                .map(|item| ItemColors {
                    id: item.id.into(),
                    label: Some(c[item.index as usize].0),
                    value: Some(c[item.index as usize].1),
                })
                .collect()
        } else {
            Vec::new()
        },
        swap_up_down: ini.get_bool("task_bar", "task_bar_swap_up_down", false),
        label_color: Some(c[0].0),
        value_color: Some(c[0].1),
        align_right: ini.get_bool("task_bar", "value_right_align", true),
        show_labels: ini.get_bool("rust_taskbar", "show_labels", true),
        show_graph: ini.get_bool("task_bar", "show_netspeed_figure", true),
        show_status_bar: ini.get_bool("task_bar", "show_status_bar", true),
        tooltip: ini.get_bool("task_bar", "show_tool_tip", true),
        place_left: ini.get_bool("task_bar", "task_bar_wnd_on_left", false),
        auto_background: ini.get_bool("task_bar", "auto_set_background_color", false),
        disable_d2d: ini.get_bool("task_bar", "disable_d2d", true),
        colorful_emoji: ini.get_bool("task_bar", "enable_colorful_emoji", true),
        snap_to_icons: ini.get_bool("task_bar", "task_bar_wnd_snap", false),
        show_graph_dashed_box: ini.get_bool("task_bar", "show_graph_dashed_box", false),
        avoid_widgets: ini.get_bool("task_bar", "avoid_overlap_with_widgets", false),
        left_space: int(ini, "task_bar", "taskbar_left_space_win11", 160, 0, 4096) as i32,
        right_space: int(ini, "task_bar", "taskbar_right_space_win11", 280, 0, 4096) as i32,
    };
    o.double_click = action(ini.get_i64("config", "double_click_action", 0));
    o.taskbar_double_click = action(ini.get_i64("task_bar", "double_click_action", 0));
    if let Some(source) = source {
        let mut skin = source.clone();
        skin.apply_overrides(ini);
        let color_order = skin_order(&skin, plugin_order);
        let f = skin.font.clone();
        o.font_name = if f.name.is_empty() {
            o.font_name
        } else {
            f.name
        };
        o.font_size = if (5..=72).contains(&f.size) {
            f.size
        } else {
            10
        };
        o.font_style = f.style & 15;
        let more = ini.get_bool("config", "show_cpu_memory", false);
        let layout = skin.layout(more);
        o.width = layout.width;
        o.height = layout.height;
        o.foreground = skin.color(0);
        let mut items = layout
            .items
            .iter()
            .filter(|(id, i)| i.show && color_order.contains(i.plugin_id.as_ref().unwrap_or(id)))
            .collect::<Vec<_>>();
        items.sort_by_key(|(id, _)| {
            DISPLAY_ITEMS
                .iter()
                .find(|i| i.id == id.as_str())
                .map(|i| usize::from(i.index))
                .unwrap_or(13)
        });
        o.visible_items = items
            .iter()
            .map(|(id, i)| i.plugin_id.clone().unwrap_or_else(|| (*id).clone()))
            .collect();
        o.skin = Some(SkinLayout {
            mask: if more {
                skin.mask_large.clone()
            } else {
                skin.mask_small.clone()
            },
            background: if more {
                skin.background_large.clone()
            } else {
                skin.background_small.clone()
            },
            width: layout.width,
            height: layout.height,
            items: items
                .into_iter()
                .map(|(id, i)| {
                    let index = color_order
                        .iter()
                        .position(|key| key == i.plugin_id.as_ref().unwrap_or(id))
                        .unwrap_or(0);
                    SkinItem {
                        id: i.plugin_id.clone().unwrap_or_else(|| id.clone()),
                        rect: [
                            i.x,
                            i.y,
                            i.x.saturating_add(i.width),
                            i.y.saturating_add(skin.text_height),
                        ],
                        color: Some(skin.color(index)),
                        align_right: i.align == Alignment::Right,
                        alignment: match i.align {
                            Alignment::Left => TextAlignment::Left,
                            Alignment::Right => TextAlignment::Right,
                            Alignment::Center => TextAlignment::Center,
                            Alignment::Side => TextAlignment::Side,
                        },
                        show_label: !skin.no_label,
                    }
                })
                .collect(),
        });
    }
    o
}

pub fn save_options(ini: &mut IniDocument, o: &DesktopOptions) {
    ini.set_bool("histroy_traffic", "use_log_scale", o.history_log_scale);
    if let Some(name) = o
        .tray_icon
        .as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
    {
        if let Some(index) = [
            "notifyicon.ico",
            "notifyicon2.ico",
            "notifyicon3.ico",
            "TrafficMonitor.ico",
            "notifyicon4.ico",
            "notifyicon5.ico",
        ]
        .iter()
        .position(|icon| icon.eq_ignore_ascii_case(name))
        {
            ini.set_i64("config", "notify_icon_selected", index as i64);
        }
    }
    for (k, v) in [
        ("hide_main_window", !o.show_main),
        ("show_task_bar_wnd", o.show_taskbar),
        ("show_notify_icon", o.show_tray),
        ("always_on_top", o.topmost),
        ("mouse_penetrate", o.click_through),
        ("lock_window_pos", o.lock_position),
        ("alow_out_of_border", !o.snap_to_edges),
        ("hide_main_wnd_when_fullscreen", o.hide_fullscreen),
        ("show_tool_tip", o.tooltip),
        ("swap_up_down", o.swap_up_down),
    ] {
        ini.set_bool("config", k, v);
    }
    for (k, v) in [
        ("transparency", (i64::from(o.opacity) * 100 + 127) / 255),
        ("position_x", o.x.into()),
        ("position_y", o.y.into()),
    ] {
        ini.set_i64("config", k, v);
    }
    ini.set_i64("general", "monitor_time_span", o.interval_ms.into());
    ini.set_i64(
        "config",
        "double_click_action",
        action_value(o.double_click),
    );
    ini.set_i64(
        "task_bar",
        "double_click_action",
        action_value(o.taskbar_double_click),
    );
    if o.skin.is_none() {
        FontInfo {
            name: o.font_name.clone(),
            size: o.font_size,
            style: o.font_style & 15,
        }
        .save(ini, "rust_main");
        ini.set_i64("rust_main", "foreground", o.foreground.into());
        ini.set_i64("rust_main", "background", o.background.into());
        ini.set("rust_main", "visible_items", o.visible_items.join(","));
        ini.set_bool(
            "config",
            "show_cpu_memory",
            o.visible_items
                .iter()
                .any(|id| matches!(id.as_str(), "cpu" | "memory")),
        );
    }
    let old_follow = ini.get_bool("task_bar", "auto_adapt_light_theme", false)
        || ini.get_bool("skins", "skin_auto_adapt", false);
    if old_follow != o.follow_system_theme {
        ini.set_bool("task_bar", "auto_adapt_light_theme", o.follow_system_theme);
        ini.set_bool("skins", "skin_auto_adapt", o.follow_system_theme);
    }
    ini.set_i64("rust_taskbar", "width", o.taskbar_width.into());
    ini.set_bool("rust_taskbar", "auto_width", o.taskbar_style.auto_width);
    ini.set_string_list("rust_taskbar", "item_order", &o.taskbar_style.visible_items);
    ini.set_i64(
        "task_bar",
        "digits_number",
        i64::from(o.taskbar_style.value_digits),
    );
    ini.set_i64("task_bar", "window_offset_left", o.taskbar_offset.into());
    ini.set_bool("task_bar", "horizontal_arrange", !o.taskbar_two_rows);
    ini.set_i64("task_bar", "item_space", o.item_gap.into());
    ini.set_bool(
        "task_bar",
        "show_taskbar_wnd_in_secondary_display",
        o.taskbar_monitor != 0,
    );
    ini.set_i64(
        "task_bar",
        "secondary_display_index",
        o.taskbar_monitor.saturating_sub(1).min(i64::MAX as usize) as i64,
    );
    let s = &o.taskbar_style;
    FontInfo {
        name: s.font_name.clone(),
        size: s.font_size,
        style: s.font_style & 15,
    }
    .save(ini, "task_bar");
    ini.set_i64("task_bar", "task_bar_back_color", s.background.into());
    ini.set_i64("rust_taskbar", "opacity", s.opacity.into());
    ini.set_bool("rust_taskbar", "show_labels", s.show_labels);
    ini.set_i64("task_bar", "window_offset_top", s.top_offset.into());
    ini.set_i64("task_bar", "vertical_margin", s.vertical_margin.into());
    if let Some(c) = s.transparent_color {
        ini.set_i64("task_bar", "transparent_color", c.into());
        ini.set_bool("other", "taksbar_transparent_color_enable", true);
    } else {
        ini.set_bool("other", "taksbar_transparent_color_enable", false);
    }
    if let Some(c) = s.graph_color {
        if !ini.get_bool("task_bar", "graph_color_following_system", true)
            || c != graph_color(ini, o.dark)
        {
            ini.set_i64("task_bar", "status_bar_color", c.into());
            ini.set_bool("task_bar", "graph_color_following_system", false);
        }
    }
    if let Some(max) = s.graph_maximum.filter(|max| max.is_finite() && *max > 0.0) {
        let unit = if max % 1048576.0 == 0.0 { 1 } else { 0 };
        let divisor = if unit == 1 { 1048576.0 } else { 1024.0 };
        ini.set_i64(
            "task_bar",
            "netspeed_figure_max_value",
            (max / divisor).round().clamp(1.0, i32::MAX as f64) as i64,
        );
        ini.set_i64("task_bar", "netspeed_figure_max_value_unit", unit);
    }
    for (k, v) in [
        ("show_netspeed_figure", s.show_graph),
        ("show_status_bar", s.show_status_bar),
        ("show_tool_tip", s.tooltip),
        ("task_bar_wnd_on_left", s.place_left),
        ("value_right_align", s.align_right),
        ("task_bar_swap_up_down", s.swap_up_down),
        ("cm_graph_type", s.graph_style == GraphStyle::Bar),
        ("task_bar_wnd_snap", s.snap_to_icons),
        ("show_graph_dashed_box", s.show_graph_dashed_box),
        ("avoid_overlap_with_widgets", s.avoid_widgets),
        ("auto_set_background_color", s.auto_background),
        ("disable_d2d", s.disable_d2d),
        ("enable_colorful_emoji", s.colorful_emoji),
    ] {
        ini.set_bool("task_bar", k, v);
    }
    ini.set_i64("task_bar", "taskbar_left_space_win11", s.left_space.into());
    ini.set_i64(
        "task_bar",
        "taskbar_right_space_win11",
        s.right_space.into(),
    );
    let mask = DISPLAY_ITEMS
        .iter()
        .filter(|i| s.visible_items.iter().any(|id| id == i.id))
        .fold(0i64, |mask, i| mask | (1 << i.index));
    ini.set_i64("task_bar", "tbar_display_item", mask);
    ini.set(
        "task_bar",
        "item_order",
        s.visible_items
            .iter()
            .filter_map(|id| {
                DISPLAY_ITEMS
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| i.index.to_string())
            })
            .collect::<Vec<_>>()
            .join(","),
    );
    ini.set(
        "task_bar",
        "plugin_display_item",
        s.visible_items
            .iter()
            .filter(|id| !DISPLAY_ITEMS.iter().any(|i| i.id == id.as_str()))
            .cloned()
            .collect::<Vec<_>>()
            .join(","),
    );
    let mut c = colors(ini);
    if s.item_colors.is_empty() {
        c[0] = (
            s.label_color.unwrap_or(s.foreground),
            s.value_color.unwrap_or(s.foreground),
        );
        ini.set_bool("task_bar", "specify_each_item_color", false);
    } else {
        for item in &s.item_colors {
            if let Some(info) = DISPLAY_ITEMS.iter().find(|i| i.id == item.id) {
                c[info.index as usize] = (
                    item.label.unwrap_or(s.foreground),
                    item.value.unwrap_or(s.foreground),
                );
            }
        }
        ini.set_bool("task_bar", "specify_each_item_color", true);
    }
    let old = ini.get_string("task_bar", "task_bar_text_color", "");
    let tail = old.split(',').skip(26).filter(|v| !v.trim().is_empty());
    let mut saved = c
        .into_iter()
        .flat_map(|(a, b)| [a.to_string(), b.to_string()])
        .collect::<Vec<_>>();
    saved.extend(tail.map(str::to_owned));
    ini.set("task_bar", "task_bar_text_color", saved.join(","));
    save_theme_preset(ini, o.dark);
}
/// The floating monitor's font/colors belong to the selected skin, not retired config keys.
pub fn save_skin_options(ini: &mut IniDocument, skin_name: &str, o: &DesktopOptions) {
    save_skin_options_with_plugin_order(ini, skin_name, o, &[])
}
pub fn save_skin_options_with_plugin_order(
    ini: &mut IniDocument,
    skin_name: &str,
    o: &DesktopOptions,
    plugins: &[String],
) {
    let section = format!(
        "skin_{}",
        traffic_monitor_config::normalize_skin_name(skin_name)
    );
    FontInfo {
        name: o.font_name.clone(),
        size: o.font_size,
        style: o.font_style & 15,
    }
    .save(ini, &section);
    let source = Skin::load(
        o.resource_dir
            .join("skins")
            .join(traffic_monitor_config::normalize_skin_name(skin_name)),
    )
    .ok();
    let order = source
        .as_ref()
        .map(|s| skin_order(s, plugins))
        .unwrap_or_else(|| {
            let mut order = o
                .skin
                .as_ref()
                .map(|s| s.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>())
                .unwrap_or_default();
            order.sort_by_key(|id| {
                DISPLAY_ITEMS
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| usize::from(i.index))
                    .unwrap_or(13)
            });
            order
        });
    let old = ini.get_string(&section, "text_color", "");
    let mut c: Vec<u32> = old
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect();
    if c.is_empty() {
        if let Some(source) = &source {
            c = source.colors.clone();
        }
    }
    c.resize(c.len().max(order.len()).max(1), o.foreground);
    let unified_changed = c.first().is_some_and(|color| *color != o.foreground);
    if unified_changed {
        c.fill(o.foreground);
    }
    if let Some(layout) = &o.skin {
        for item in &layout.items {
            if let Some(index) = order.iter().position(|id| id == &item.id) {
                let color = item.color.unwrap_or(o.foreground);
                c[index] = color;
            }
        }
    } else {
        c.fill(o.foreground);
    }
    ini.set(
        &section,
        "text_color",
        c.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
    );
    ini.set_bool(
        &section,
        "specify_each_item_color",
        c.iter().any(|color| *color != o.foreground),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn theme_graph_color_preserves_hue_and_adjusts_lightness() {
        assert_eq!(themed_graph_color(0x0000ff, false), 0x0000cc);
        assert_eq!(themed_graph_color(0x000000, false), 0x666666);
        assert_eq!(themed_graph_color(0xffffff, false), 0x666666);
        assert_eq!(themed_graph_color(0x0000ff, true), 0x6666ff);
    }
}
