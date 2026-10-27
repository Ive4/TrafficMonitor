use crate::*;
pub(crate) fn taskbar_panel_width(needed: i32, fixed_minimum: Option<i32>, available: i32) -> i32 {
    if needed <= 0 || available <= 0 {
        return 0;
    }
    needed.max(fixed_minimum.unwrap_or(0)).clamp(1, available)
}
pub(crate) fn taskbar_measurement_value(value: &str, digits: u8) -> String {
    let end = value
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit() && !matches!(c, '.' | ',' | '+' | '-'))
        .map_or(value.len(), |(i, _)| i);
    let prefix = &value[..end];
    if !prefix.chars().any(|c| c.is_ascii_digit()) {
        return value.into();
    }
    let suffix = &value[end..];
    let suffix = if suffix.contains("/s") {
        suffix.replace(['K', 'G'], "M")
    } else {
        suffix.into()
    };
    format!(
        "{}{suffix}",
        "8".repeat(prefix.len().max(usize::from(digits.min(12))))
    )
}
pub(crate) fn monitor_bounds(
    o: &DesktopOptions,
    dpi: u32,
    suggested: Option<[i32; 4]>,
) -> [i32; 4] {
    suggested.unwrap_or_else(|| {
        [
            o.x,
            o.y,
            o.x + o.skin.as_ref().map_or(o.width, |s| s.width) * dpi as i32 / 96,
            o.y + o.skin.as_ref().map_or(o.height, |s| s.height) * dpi as i32 / 96,
        ]
    })
}
/// Legacy cm_graph_type selects historical bars or a current-value progress bar.
pub(crate) fn graph_policy(item: &DisplayValue, style: &TaskbarStyle) -> Option<(bool, f64)> {
    let network = matches!(
        item.id.as_str(),
        "up" | "down" | "upload" | "download" | "total_speed"
    );
    let status = item.plugin
        || matches!(
            item.id.as_str(),
            "cpu"
                | "memory"
                | "gpu"
                | "hdd"
                | "cpu_temperature"
                | "gpu_temperature"
                | "hdd_temperature"
                | "main_board_temperature"
        );
    if !(if network {
        style.show_graph
    } else {
        status && style.show_status_bar
    }) {
        return None;
    }
    let maximum = if network {
        style
            .graph_maximum
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or_else(|| {
                item.history
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .fold(1.0, f64::max)
            })
    } else {
        100.0
    };
    Some((style.graph_style == GraphStyle::Bar, maximum))
}
pub(crate) fn history_chart_rect(dpi: u32, scroll_x: i32, scroll_y: i32) -> [i32; 4] {
    [
        20 * dpi as i32 / 96 - scroll_x,
        470 * dpi as i32 / 96 - scroll_y,
        900 * dpi as i32 / 96 - scroll_x,
        680 * dpi as i32 / 96 - scroll_y,
    ]
}
pub(crate) fn parse_list_csv(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut was_quoted = false;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => {
                quoted = !quoted;
                was_quoted = true;
            }
            ',' if !quoted => {
                let v = if was_quoted {
                    field.clone()
                } else {
                    field.trim().to_string()
                };
                if !v.is_empty() {
                    result.push(v);
                }
                field.clear();
                was_quoted = false;
            }
            _ => field.push(c),
        }
    }
    let v = if was_quoted {
        field
    } else {
        field.trim().to_string()
    };
    if !v.is_empty() {
        result.push(v);
    }
    result
}
pub(crate) fn encode_list_csv(values: &[String]) -> String {
    values
        .iter()
        .map(|v| {
            if v.contains([',', '"', '\r', '\n']) || v.trim() != v {
                format!("\"{}\"", v.replace('"', "\"\""))
            } else {
                v.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}
pub(crate) fn font_height(points: i32, dpi: u32) -> i32 {
    -((points * dpi as i32 + 36) / 72)
}
pub(crate) fn ordered_items(o: &DesktopOptions, d: &Dashboard, taskbar: bool) -> Vec<DisplayValue> {
    let source = if taskbar && !d.taskbar_items.is_empty() {
        &d.taskbar_items
    } else {
        &d.items
    };
    let ids = if taskbar {
        &o.taskbar_style.visible_items
    } else {
        &o.visible_items
    };
    let mut items: Vec<_> = if taskbar && ids.is_empty() {
        Vec::new()
    } else if ids.is_empty() {
        source.clone()
    } else {
        ids.iter()
            .filter_map(|id| source.iter().find(|i| &i.id == id).cloned())
            .collect()
    };
    if if taskbar {
        o.taskbar_style.swap_up_down
    } else {
        o.swap_up_down
    } {
        let up = items
            .iter()
            .position(|i| matches!(i.id.as_str(), "upload" | "up"));
        let down = items
            .iter()
            .position(|i| matches!(i.id.as_str(), "download" | "down"));
        if let (Some(up), Some(down)) = (up, down) {
            items.swap(up, down);
        }
    }
    items
}
pub(crate) fn taskbar_item_layout(
    widths: &[(i32, bool)],
    height: i32,
    two_rows: bool,
    gap: i32,
) -> (Vec<[i32; 4]>, i32) {
    let mut rects = vec![[0; 4]; widths.len()];
    let mut x = 0;
    let mut i = 0;
    while i < widths.len() {
        let count = if two_rows && !widths[i].1 && i + 1 < widths.len() && !widths[i + 1].1 {
            2
        } else {
            1
        };
        let width = widths[i..i + count]
            .iter()
            .map(|v| v.0.max(1))
            .max()
            .unwrap_or(1);
        for row in 0..count {
            let full = !two_rows || widths[i].1;
            let row_height = if full { height } else { height / 2 };
            rects[i + row] = [
                x + gap,
                row as i32 * row_height,
                x + gap + width,
                if full {
                    height
                } else {
                    (row as i32 + 1) * row_height
                },
            ];
        }
        x += width + gap * 2;
        i += count;
    }
    (rects, x)
}
pub(crate) fn validate_options(o: &DesktopOptions) -> Result<(), String> {
    if !o.show_main && !o.show_taskbar && !o.show_tray {
        return Err("Keep a monitor or notification icon visible".into());
    }
    if o.click_through && !o.show_tray && !o.show_taskbar {
        return Err("Click-through requires a notification icon or taskbar monitor".into());
    }
    if !(5..=72).contains(&o.taskbar_style.font_size)
        || o.font_style > 15
        || o.taskbar_style.font_style > 15
    {
        return Err("Font size must be 5–72; style flags must be 0–15".into());
    }
    if !(100..=60000).contains(&o.interval_ms) {
        return Err("Refresh interval must be 100–60000 milliseconds".into());
    }
    if !o.show_tray
        && !(o.show_main && o.opacity > 0 && !o.click_through)
        && !(o.show_taskbar && o.taskbar_style.opacity > 0)
    {
        return Err(
            "Keep a visible, interactive monitor or notification icon to restore the application"
                .into(),
        );
    }
    if !(1..=4096).contains(&o.width)
        || !(1..=2160).contains(&o.height)
        || !(5..=72).contains(&o.font_size)
    {
        return Err("Window size or font size is outside the supported range".into());
    }
    if !(1..=4096).contains(&o.taskbar_width) || o.item_gap < 0 || o.item_gap > 100 {
        return Err("Invalid taskbar width or item gap".into());
    }
    Ok(())
}
pub(crate) fn item_rects(
    o: &DesktopOptions,
    ids: &[String],
    width: i32,
    height: i32,
    taskbar: bool,
) -> Vec<[i32; 4]> {
    let rows = if taskbar && !o.taskbar_two_rows {
        1
    } else if taskbar {
        2
    } else {
        ids.len().clamp(1, 4)
    };
    let cols = ids.len().div_ceil(rows).max(1);
    let cell_w = width / cols as i32;
    let cell_h = height / rows as i32;
    ids.iter()
        .enumerate()
        .map(|(i, id)| {
            if !taskbar {
                if let Some(s) = &o.skin {
                    if let Some(item) = s.items.iter().find(|v| &v.id == id) {
                        let r = item.rect;
                        return [
                            r[0] * width / s.width.max(1),
                            r[1] * height / s.height.max(1),
                            r[2] * width / s.width.max(1),
                            r[3] * height / s.height.max(1),
                        ];
                    }
                }
            }
            let x = (i / rows) as i32 * cell_w;
            let y = (i % rows) as i32 * cell_h;
            [
                x + o.item_gap.min(cell_w / 4),
                y,
                x + cell_w - o.item_gap.min(cell_w / 4),
                y + cell_h,
            ]
        })
        .collect()
}
pub(crate) fn snap(rect: [i32; 4], work: [i32; 4], distance: i32) -> (i32, i32) {
    let mut x = rect[0];
    let mut y = rect[1];
    if (x - work[0]).abs() <= distance {
        x = work[0]
    } else if (rect[2] - work[2]).abs() <= distance {
        x = work[2] - (rect[2] - rect[0])
    }
    if (y - work[1]).abs() <= distance {
        y = work[1]
    } else if (rect[3] - work[3]).abs() <= distance {
        y = work[3] - (rect[3] - rect[1])
    }
    (x, y)
}
pub(crate) fn chart_height(value: f64, maximum: f64, height: i32, log: bool) -> i32 {
    if !value.is_finite() || !maximum.is_finite() || value <= 0.0 || maximum <= 0.0 {
        return 0;
    }
    let ratio = if log {
        value.ln_1p() / maximum.ln_1p()
    } else {
        value / maximum
    };
    (ratio.clamp(0.0, 1.0) * f64::from(height.max(0))).round() as i32
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_taskbar_width_has_no_default_blank_reservation() {
        assert_eq!(taskbar_panel_width(76, None, 1920), 76);
        assert_eq!(taskbar_panel_width(120, None, 1920), 120);
        assert_eq!(taskbar_panel_width(76, Some(180), 1920), 180);
        assert_eq!(taskbar_panel_width(200, Some(100), 1920), 200);
        assert_eq!(taskbar_panel_width(200, None, 150), 150);
        assert_eq!(taskbar_panel_width(0, Some(180), 1920), 0);
    }
    #[test]
    fn measurement_reserves_digits_and_stabilizes_auto_speed_units() {
        assert_eq!(taskbar_measurement_value("0.00 K/s", 4), "8888 M/s");
        assert_eq!(taskbar_measurement_value("998 M/s", 4), "8888 M/s");
        assert_eq!(taskbar_measurement_value("10240 KB/s", 4), "88888 MB/s");
        assert_eq!(taskbar_measurement_value("--", 4), "--");
        assert_eq!(taskbar_measurement_value("75 %", 3), "888 %");
    }
    #[test]
    fn deselecting_all_taskbar_items_does_not_render_every_metric() {
        let mut o = DesktopOptions::default();
        o.taskbar_style.visible_items.clear();
        let d = Dashboard {
            items: vec![
                DisplayValue {
                    id: "up".into(),
                    ..Default::default()
                },
                DisplayValue {
                    id: "cpu".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert!(ordered_items(&o, &d, true).is_empty());
        assert_eq!(ordered_items(&o, &d, false).len(), 2);
    }
    #[test]
    fn dpi_transition_uses_suggested_monitor_position_before_saved_position() {
        let o = DesktopOptions {
            x: 100,
            y: 100,
            ..Default::default()
        };
        let suggested = [-1500, 200, -900, 400];
        assert_eq!(monitor_bounds(&o, 192, Some(suggested)), suggested);
        assert_eq!(monitor_bounds(&o, 192, None)[0..2], [100, 100]);
    }
    #[test]
    fn graphs_use_independent_switches_and_metric_ranges() {
        let mut style = TaskbarStyle {
            show_graph: false,
            show_status_bar: true,
            graph_style: GraphStyle::Bar,
            graph_maximum: Some(10.0 * 1024.0 * 1024.0),
            ..Default::default()
        };
        let cpu = DisplayValue {
            id: "cpu".into(),
            history: vec![50.0, 100.0],
            ..Default::default()
        };
        let network = DisplayValue {
            id: "up".into(),
            history: vec![1024.0, 2048.0],
            ..Default::default()
        };
        assert_eq!(graph_policy(&cpu, &style), Some((true, 100.0)));
        assert_eq!(graph_policy(&network, &style), None);
        style.show_status_bar = false;
        style.show_graph = true;
        style.graph_style = GraphStyle::Line;
        assert_eq!(graph_policy(&cpu, &style), None);
        assert_eq!(
            graph_policy(&network, &style),
            Some((false, 10.0 * 1024.0 * 1024.0))
        );
    }
    #[test]
    fn history_graph_scales_and_scrolls_with_controls() {
        assert_eq!(history_chart_rect(144, 30, 90), [0, 615, 1320, 930]);
        assert_eq!(history_chart_rect(192, 40, 120), [0, 820, 1760, 1240]);
    }
    #[test]
    fn list_csv_preserves_quoted_commas_quotes_and_unicode() {
        let values = vec![
            "Ethernet, USB".to_string(),
            "无线网络".to_string(),
            "quoted \"name\"".to_string(),
            " spaces ".to_string(),
        ];
        assert_eq!(parse_list_csv(&encode_list_csv(&values)), values);
        assert_eq!(parse_list_csv("up, down,cpu"), vec!["up", "down", "cpu"]);
    }
    #[test]
    fn taskbar_plugin_width_and_double_line_are_respected() {
        let (r, w) = taskbar_item_layout(&[(120, true), (40, false), (60, false)], 40, true, 4);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], [4, 0, 124, 40]);
        assert_eq!(r[1][1], 0);
        assert_eq!(r[2][1], 20);
        assert_eq!(r[2][3], 40);
        assert_eq!(w, 196);
    }
    #[test]
    fn small_legacy_skins_and_zero_opacity_with_tray_remain_valid() {
        let o = DesktopOptions {
            width: 40,
            height: 16,
            opacity: 0,
            taskbar_width: 20,
            ..Default::default()
        };
        assert!(validate_options(&o).is_ok());
    }
    #[test]
    fn font_sizes_preserve_legacy_points() {
        assert_eq!(font_height(15, 96), -20);
        assert_eq!(font_height(15, 144), -30);
    }
    #[test]
    fn invalid_interval_and_opacity_are_rejected() {
        let mut o = DesktopOptions {
            interval_ms: 1,
            ..Default::default()
        };
        assert!(validate_options(&o).is_err());
        o.interval_ms = 1000;
        o.opacity = 0;
        o.show_tray = false;
        assert!(validate_options(&o).is_err());
    }
    #[test]
    fn taskbar_rows_fit_and_preserve_order() {
        let o = DesktopOptions::default();
        let r = item_rects(
            &o,
            &["up".into(), "down".into(), "cpu".into()],
            200,
            40,
            true,
        );
        assert_eq!(r.len(), 3);
        assert_eq!(r[0][1], r[2][1]);
        assert!(r[1][1] > r[0][1]);
        assert!(r.iter().all(|r| r[2] <= 200 && r[3] <= 40));
    }
    #[test]
    fn skin_coordinates_override_automatic_layout() {
        let o = DesktopOptions {
            skin: Some(SkinLayout {
                background: None,
                mask: None,
                width: 300,
                height: 100,
                items: vec![SkinItem {
                    id: "up".into(),
                    rect: [5, 6, 80, 30],
                    color: None,
                    align_right: false,
                    alignment: TextAlignment::Left,
                    show_label: false,
                }],
            }),
            ..Default::default()
        };
        assert_eq!(
            item_rects(&o, &["up".into()], 300, 100, false),
            vec![[5, 6, 80, 30]]
        );
    }
    #[test]
    fn snap_respects_negative_monitor_coordinates() {
        assert_eq!(
            snap([-1915, 12, -1615, 112], [-1920, 0, 0, 1080], 16),
            (-1920, 0)
        );
    }
    #[test]
    fn graph_handles_missing_and_log_data() {
        assert_eq!(chart_height(f64::NAN, 1.0, 100, false), 0);
        assert_eq!(chart_height(10.0, 100.0, 100, false), 10);
        assert!(chart_height(10.0, 100.0, 100, true) > 40);
    }
    #[test]
    fn inaccessible_options_and_invalid_taskbar_font_are_rejected() {
        let mut o = DesktopOptions {
            show_main: false,
            show_taskbar: false,
            show_tray: false,
            ..Default::default()
        };
        assert!(validate_options(&o).is_err());
        o.show_main = true;
        o.taskbar_style.font_size = 0;
        assert!(validate_options(&o).is_err());
    }
    #[test]
    fn skin_item_rectangles_scale_with_monitor_dpi() {
        let o = DesktopOptions {
            skin: Some(SkinLayout {
                background: None,
                mask: None,
                width: 300,
                height: 100,
                items: vec![SkinItem {
                    id: "up".into(),
                    rect: [10, 10, 150, 30],
                    color: None,
                    align_right: false,
                    alignment: TextAlignment::Center,
                    show_label: true,
                }],
            }),
            ..Default::default()
        };
        assert_eq!(
            item_rects(&o, &["up".into()], 600, 200, false),
            vec![[20, 20, 300, 60]]
        );
    }
}
