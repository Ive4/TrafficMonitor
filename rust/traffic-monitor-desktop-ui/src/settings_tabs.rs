//! Stable setting categories and value comparisons used by the native tabbed editor.
use crate::{DisplayValue, SettingField, SettingKind};

pub(crate) fn validate_fields(
    values: &[(String, String)],
    fields: &[SettingField],
) -> Result<(), String> {
    for (key, value) in values {
        if let Some(field) = fields.iter().find(|f| &f.key == key) {
            if field.kind == SettingKind::Integer {
                let valid = value.trim().parse::<i64>().ok().is_some_and(|n| {
                    if let Some((min, max)) = field.help.split_once('–') {
                        n >= min.parse().unwrap_or(i64::MIN) && n <= max.parse().unwrap_or(i64::MAX)
                    } else {
                        true
                    }
                });
                if !valid {
                    return Err(format!("{}: {}", field.label, field.help));
                }
            }
            if field.kind == SettingKind::Color
                && !value
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .is_some_and(|n| n <= 0xffffff)
            {
                return Err(format!("{}: 0–16777215", field.label));
            }
            if let SettingKind::Choice(choices) = &field.kind {
                if !choices.contains(value) {
                    return Err(field.label.clone());
                }
            }
            if key == "general/connections_hide" && value.contains(['\r', '\n']) {
                return Err(field.label.clone());
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    General,
    Floating,
    Taskbar,
    Items,
    Alerts,
    Advanced,
    Appearance,
}
impl SettingsTab {
    pub(crate) const ALL: [Self; 7] = [
        Self::General,
        Self::Floating,
        Self::Taskbar,
        Self::Items,
        Self::Alerts,
        Self::Advanced,
        Self::Appearance,
    ];
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Floating => "Floating window",
            Self::Taskbar => "Taskbar",
            Self::Items => "Display items",
            Self::Alerts => "Alerts",
            Self::Advanced => "Advanced settings",
            Self::Appearance => "Skins and language",
        }
    }
}
pub(crate) fn category(key: &str) -> SettingsTab {
    if key.starts_with("notify_tip/") || key == "other/notify_interval" {
        SettingsTab::Alerts
    } else if key.starts_with("task_bar/") {
        SettingsTab::Taskbar
    } else if key.starts_with("skins/")
        || matches!(key, "general/language" | "config/skin_selected")
    {
        SettingsTab::Appearance
    } else if key.starts_with("general/") || key.starts_with("connection/") {
        SettingsTab::General
    } else if key.starts_with("config/") {
        SettingsTab::Floating
    } else {
        SettingsTab::Advanced
    }
}
pub(crate) fn changed_values(
    values: &[(String, String)],
    baseline: &[(String, String)],
) -> Vec<(String, String)> {
    values
        .iter()
        .filter(|(key, value)| {
            !baseline
                .iter()
                .any(|(old_key, old_value)| old_key == key && old_value == value)
        })
        .cloned()
        .collect()
}
pub(crate) fn item_catalog(items: &[DisplayValue]) -> Vec<(String, String)> {
    let mut result: Vec<_> = [
        ("up", "Upload (B/s)"),
        ("down", "Download (B/s)"),
        ("cpu", "CPU usage (%)"),
        ("memory", "Memory (%)"),
        ("gpu", "GPU usage (%)"),
        ("cpu_temperature", "CPU temperature (°C)"),
        ("gpu_temperature", "GPU temperature (°C)"),
        ("hdd_temperature", "Disk temperature (°C)"),
        ("main_board_temperature", "Motherboard temperature (°C)"),
        ("hdd", "Disk activity (%)"),
        ("total_speed", "Total speed (B/s)"),
        ("cpu_freq", "CPU frequency (GHz)"),
        ("today_traffic", "Today traffic (B)"),
    ]
    .into_iter()
    .map(|(id, label)| (id.to_owned(), label.to_owned()))
    .collect();
    for item in items {
        if !result.iter().any(|(id, _)| id == &item.id) {
            result.push((
                item.id.clone(),
                if item.label.trim().is_empty() {
                    item.id.clone()
                } else {
                    item.label.clone()
                },
            ));
        }
    }
    result
}
pub(crate) fn covered_by_native_editor(key: &str) -> bool {
    matches!(
        key,
        "general/language"
            | "general/monitor_time_span"
            | "config/transparency"
            | "config/always_on_top"
            | "config/lock_window_pos"
            | "config/show_notify_icon"
            | "config/mouse_penetrate"
            | "config/show_task_bar_wnd"
            | "config/hide_main_window"
            | "config/skin_selected"
            | "config/swap_up_down"
            | "config/hide_main_wnd_when_fullscreen"
            | "config/show_tool_tip"
            | "config/double_click_action"
            | "task_bar/font_name"
            | "task_bar/font_size"
            | "task_bar/font_style"
            | "task_bar/task_bar_back_color"
            | "task_bar/transparent_color"
            | "task_bar/tbar_display_item"
            | "task_bar/item_order"
            | "task_bar/plugin_display_item"
            | "task_bar/task_bar_wnd_on_left"
            | "task_bar/value_right_align"
            | "task_bar/horizontal_arrange"
            | "task_bar/show_status_bar"
            | "task_bar/show_tool_tip"
            | "task_bar/double_click_action"
            | "task_bar/cm_graph_type"
            | "task_bar/item_space"
            | "task_bar/vertical_margin"
            | "task_bar/window_offset_top"
            | "task_bar/window_offset_left"
            | "task_bar/show_netspeed_figure"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_are_grouped_by_behavior_instead_of_field_count() {
        assert_eq!(
            category("notify_tip/cpu_temperature_tip_value"),
            SettingsTab::Alerts
        );
        assert_eq!(category("other/notify_interval"), SettingsTab::Alerts);
        assert_eq!(category("task_bar/disable_d2d"), SettingsTab::Taskbar);
        assert_eq!(category("config/speed_unit"), SettingsTab::Floating);
        assert_eq!(
            category("general/cpu_usage_acquire_method"),
            SettingsTab::General
        );
    }
    #[test]
    fn switching_categories_does_not_turn_untouched_settings_into_writes() {
        let baseline = vec![
            ("notify_tip/traffic_tip_value".into(), "200".into()),
            ("interval_ms".into(), "1000".into()),
        ];
        assert!(changed_values(&baseline, &baseline).is_empty());
        let mut edited = baseline.clone();
        edited[1].1 = "1234".into();
        assert_eq!(
            changed_values(&edited, &baseline),
            vec![("interval_ms".into(), "1234".into())]
        );
    }
    #[test]
    fn checkbox_catalog_contains_every_builtin_and_loaded_plugin_once() {
        let plugin = DisplayValue {
            id: "weather.temp".into(),
            label: "Weather temperature".into(),
            plugin: true,
            ..Default::default()
        };
        let items = item_catalog(&[plugin.clone(), plugin]);
        assert_eq!(items.len(), 14);
        assert!(
            items
                .iter()
                .any(|(id, label)| id == "cpu_temperature" && label.contains("°C"))
        );
        assert!(
            items
                .iter()
                .any(|(id, label)| id == "weather.temp" && label == "Weather temperature")
        );
    }
    #[test]
    fn invalid_color_rejects_the_whole_preferences_batch_before_dispatch() {
        let fields = vec![SettingField {
            key: "task_bar/status_bar_color".into(),
            label: "Graph color".into(),
            value: "0".into(),
            kind: SettingKind::Color,
            help: String::new(),
        }];
        assert!(
            validate_fields(
                &[
                    ("interval_ms".into(), "2000".into()),
                    ("task_bar/status_bar_color".into(), "bad".into())
                ],
                &fields
            )
            .is_err()
        );
        assert!(
            validate_fields(
                &[("task_bar/status_bar_color".into(), "16777215".into())],
                &fields
            )
            .is_ok()
        );
    }
}
