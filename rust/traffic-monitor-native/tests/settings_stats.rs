use std::path::{Path, PathBuf};
use traffic_monitor_config::{IniDocument, Skin};
use traffic_monitor_desktop_ui::{DoubleClickAction, HistoryGrouping};
use traffic_monitor_history::{Date, HistoryDocument, HistoryRecord};
use traffic_monitor_native::{settings, stats};
fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn split(y: u16, m: u8, d: u8, up: u64, down: u64) -> HistoryRecord {
    HistoryRecord::split(Date::new(y, m, d).unwrap(), up, down).unwrap()
}

#[test]
fn taskbar_plugin_order_and_width_preferences_roundtrip() {
    let ini = IniDocument::default();
    let mut options = settings::options(&ini, Path::new("assets"), None, true);
    options.taskbar_style.visible_items = vec![
        "weather.temp".into(),
        "up".into(),
        "clock".into(),
        "cpu".into(),
    ];
    options.taskbar_width = 2400;
    options.taskbar_style.auto_width = false;
    options.taskbar_style.value_digits = 8;
    let mut saved = ini.clone();
    settings::save_options(&mut saved, &options);
    let restored = settings::options(&saved, Path::new("assets"), None, true);
    assert_eq!(
        restored.taskbar_style.visible_items,
        options.taskbar_style.visible_items
    );
    assert_eq!(restored.taskbar_width, 2400);
    assert!(!restored.taskbar_style.auto_width);
    assert_eq!(restored.taskbar_style.value_digits, 8);
    let mut empty = restored.clone();
    empty.taskbar_style.visible_items.clear();
    settings::save_options(&mut saved, &empty);
    assert!(
        settings::options(&saved, Path::new("assets"), None, true)
            .taskbar_style
            .visible_items
            .is_empty()
    );
}
#[test]
fn new_taskbar_defaults_include_system_metrics_without_overriding_saved_choices() {
    let ini = IniDocument::default();
    let o = settings::options(&ini, Path::new("assets"), None, true);
    assert_eq!(
        o.taskbar_style.visible_items,
        vec!["up", "down", "cpu", "memory"]
    );
    let saved = IniDocument::parse("[task_bar]\ntbar_display_item=3\n");
    assert_eq!(
        settings::options(&saved, Path::new("assets"), None, true)
            .taskbar_style
            .visible_items,
        vec!["up", "down"]
    );
}
#[test]
fn legacy_defaults_and_sampling_bounds_are_retained() {
    let ini = IniDocument::default();
    let options = settings::options(&ini, Path::new("assets"), None, true);
    assert_eq!(options.opacity, 204);
    assert_eq!(options.font_size, 10);
    assert_eq!(options.taskbar_style.font_size, 9);
    assert_eq!(options.double_click, DoubleClickAction::Network);
    assert_eq!(options.visible_items, vec!["up", "down"]);
    assert_eq!(
        options.taskbar_style.visible_items,
        vec!["up", "down", "cpu", "memory"]
    );
    let ini = IniDocument::parse("[general]\nmonitor_time_span=1\n");
    assert_eq!(
        settings::options(&ini, Path::new(""), None, false).interval_ms,
        1000
    );
}

#[test]
fn every_legacy_double_click_value_roundtrips() {
    for (n, action) in [
        DoubleClickAction::Network,
        DoubleClickAction::History,
        DoubleClickAction::ToggleMore,
        DoubleClickAction::Settings,
        DoubleClickAction::TaskManager,
        DoubleClickAction::SpecificApp,
        DoubleClickAction::ChangeSkin,
        DoubleClickAction::None,
    ]
    .iter()
    .enumerate()
    {
        let mut ini = IniDocument::parse("[vendor]\nuntouched=yes\n");
        ini.set_i64("config", "double_click_action", n as i64);
        let o = settings::options(&ini, Path::new(""), None, false);
        assert_eq!(&o.double_click, action);
        settings::save_options(&mut ini, &o);
        assert_eq!(ini.get_i64("config", "double_click_action", -1), n as i64);
        assert_eq!(ini.get("vendor", "untouched"), Some("yes"));
    }
}

#[test]
fn real_skin_overrides_font_and_color_enum_order() {
    let mut skin = Skin::load(repo().join("TrafficMonitor/skins/xml_test")).unwrap();
    skin.colors = (1..=20).collect();
    skin.specify_each_item_color = true;
    let ini = IniDocument::parse(
        "[config]\nshow_cpu_memory=true\nfont_name=obsolete\n[skin_xml_test]\nfont_name=Custom Font\nfont_style=14\n",
    );
    let o = settings::options(&ini, Path::new(""), Some(&skin), false);
    assert_eq!(o.font_name, "Custom Font");
    assert_eq!(o.font_style, 14);
    let layout = o.skin.unwrap();
    assert_eq!(
        layout.items.iter().find(|i| i.id == "cpu").unwrap().color,
        Some(3)
    );
    assert!(layout.items.iter().any(|i| i.id == "b5R30ITQ"));
    assert_eq!(
        layout
            .items
            .iter()
            .find(|i| i.id == "total_speed")
            .unwrap()
            .color,
        Some(5)
    );
}

#[test]
fn skin_plugin_colors_use_loaded_plugin_order_and_compact_layout_union() {
    let mut skin = Skin::load(repo().join("TrafficMonitor/skins/xml_test")).unwrap();
    skin.colors = (1..=20).collect();
    skin.specify_each_item_color = true;
    let ini = IniDocument::parse("[config]\nshow_cpu_memory=true\n");
    let o = settings::options_with_plugin_order(
        &ini,
        Path::new(""),
        Some(&skin),
        false,
        &["b5R30ITQ".into()],
    );
    assert_eq!(
        o.skin
            .as_ref()
            .unwrap()
            .items
            .iter()
            .find(|i| i.id == "b5R30ITQ")
            .unwrap()
            .color,
        Some(6)
    );
    assert!(!o.visible_items.iter().any(|i| i == "NdKZEf39"));
}

#[test]
fn saving_does_not_flatten_independent_legacy_colors() {
    let mut ini = IniDocument::parse(
        "[task_bar]\nspecify_each_item_color=true\ntask_bar_text_color=1,2,3,4,5,6,7,8\nfont_style=10\n",
    );
    let o = settings::options(&ini, Path::new(""), None, false);
    settings::save_options(&mut ini, &o);
    assert_eq!(
        &ini.get_i64_array("task_bar", "task_bar_text_color", 8, 0),
        &[1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert_eq!(o.taskbar_style.font_style, 10);
}

#[test]
fn date_input_rejects_fake_dates_and_accepts_both_separators() {
    assert_eq!(
        stats::parse_date("2024-02-29").unwrap(),
        Date::new(2024, 2, 29).unwrap()
    );
    assert_eq!(
        stats::format_date(stats::parse_date("2024/02/29").unwrap()),
        "2024/02/29"
    );
    for bad in [
        "2023/02/29",
        "1900/02/29",
        "2024/13/01",
        "2024/04/31",
        "2024/01/01/02",
        "",
    ] {
        assert!(stats::parse_date(bad).is_err(), "{bad}");
    }
}

#[test]
fn history_week_crosses_year_with_correct_start_day() {
    let doc = HistoryDocument::new(vec![
        split(2023, 12, 31, 1, 2),
        split(2024, 1, 1, 3, 4),
        split(2024, 1, 6, 5, 6),
        split(2024, 1, 7, 7, 8),
    ]);
    let (sun, _) = stats::rows(&doc, "", "", HistoryGrouping::Week, true).unwrap();
    assert_eq!(sun.len(), 2);
    assert!(
        sun.iter()
            .any(|r| r.date == "2023/12/31–2024/01/06" && r.total_bytes == 21 * 1024)
    );
    let (mon, _) = stats::rows(&doc, "", "", HistoryGrouping::Week, false).unwrap();
    assert_eq!(mon.len(), 2);
    assert!(
        mon.iter()
            .any(|r| r.date == "2024/01/01–2024/01/07" && r.total_bytes == 33 * 1024)
    );
}

#[test]
fn history_mixed_totals_stay_separate_and_document_is_untouched() {
    let doc = HistoryDocument::new(vec![
        HistoryRecord::mixed(Date::new(2024, 2, 29).unwrap(), 10),
        split(2024, 2, 29, 2, 3),
    ]);
    let original = doc.clone();
    let (rows, summary) =
        stats::rows(&doc, "2024-02-29", "2024/02/29", HistoryGrouping::Day, true).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].total_bytes, 15 * 1024);
    assert_eq!(rows[0].upload, "2.00 KB");
    assert_eq!(rows[0].download, "3.00 KB");
    assert!(!rows[0].mixed.is_empty());
    assert!(summary.contains("未知方向"));
    assert_eq!(doc, original);
}

#[test]
fn history_aggregates_in_u128_and_filters_before_grouping() {
    let doc = HistoryDocument::new(vec![
        HistoryRecord::mixed(Date::new(2024, 1, 1).unwrap(), u64::MAX),
        HistoryRecord::mixed(Date::new(2024, 1, 2).unwrap(), u64::MAX),
        split(2024, 2, 1, 1, 2),
    ]);
    let (rows, _) = stats::rows(
        &doc,
        "2024/01/01",
        "2024/01/31",
        HistoryGrouping::Year,
        true,
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].total_bytes, u128::from(u64::MAX) * 2 * 1024);
    assert_eq!(rows[0].date, "2024");
    assert!(stats::rows(&doc, "2025/01/01", "2024/01/01", HistoryGrouping::Day, true).is_err());
}

#[test]
fn repository_legacy_history_fixture_is_rendered() {
    let doc = HistoryDocument::parse(include_str!(
        "../../traffic-monitor-history/tests/fixtures/history_legacy.txt"
    ))
    .unwrap();
    let (rows, summary) = stats::rows(&doc, "", "", HistoryGrouping::Month, true).unwrap();
    assert!(!rows.is_empty());
    assert!(!summary.is_empty());
}

#[test]
fn quarter_ranges_and_pure_unknown_direction_are_explicit() {
    let doc = HistoryDocument::new(vec![
        split(2024, 3, 31, 1, 2),
        HistoryRecord::mixed(Date::new(2024, 4, 1).unwrap(), 4),
    ]);
    let (rows, _) = stats::rows(&doc, "", "", HistoryGrouping::Quarter, true).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].date, "2024/Q2");
    assert_eq!(rows[0].upload, "--");
    assert_eq!(rows[0].download, "--");
    assert_eq!(rows[0].mixed, "4.00 KB");
    assert_eq!(rows[1].date, "2024/Q1");
}

#[test]
fn taskbar_extended_style_and_item_order_roundtrip_in_temporary_ini() {
    use traffic_monitor_desktop_ui::GraphStyle;
    let mut ini = IniDocument::parse(
        "[task_bar]\ntbar_display_item=15\nitem_order=2,0,1,3\nshow_taskbar_wnd_in_secondary_display=true\nsecondary_display_index=1\nwindow_offset_top=12\nwindow_offset_left=-37\nvertical_margin=-3\nfont_style=14\nnetspeed_figure_max_value=24\nnetspeed_figure_max_value_unit=0\ncm_graph_type=false\ntask_bar_back_color=1234\ntransparent_color=1234\ndouble_click_action=5\n[config]\nswap_up_down=true\nshow_tool_tip=false\n[other]\ntaksbar_transparent_color_enable=true\n[vendor]\nx=keep\n",
    );
    let mut o = settings::options(&ini, Path::new(""), None, false);
    assert_eq!(
        o.taskbar_style.visible_items,
        vec!["cpu", "up", "down", "memory"]
    );
    assert_eq!(o.taskbar_monitor, 2);
    assert_eq!(o.taskbar_style.top_offset, 12);
    assert_eq!(o.taskbar_style.vertical_margin, -3);
    assert_eq!(o.taskbar_style.graph_maximum, Some(24576.0));
    assert_eq!(o.taskbar_style.graph_style, GraphStyle::Line);
    assert_eq!(o.taskbar_style.transparent_color, Some(1234));
    assert_eq!(o.taskbar_double_click, DoubleClickAction::SpecificApp);
    assert!(o.swap_up_down);
    assert!(!o.tooltip);
    o.taskbar_style.opacity = 117;
    o.taskbar_style.swap_up_down = true;
    settings::save_options(&mut ini, &o);
    let dir = std::env::temp_dir().join(format!(
        "tm-settings-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.ini");
    ini.save_atomic(&path).unwrap();
    let saved = IniDocument::load(path).unwrap();
    let loaded = settings::options(&saved, Path::new(""), None, false);
    assert_eq!(loaded.taskbar_style, o.taskbar_style);
    assert_eq!(saved.get("vendor", "x"), Some("keep"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn saving_skin_options_is_isolated_from_retired_global_font_keys() {
    let mut ini =
        IniDocument::parse("[config]\nfont_name=old-global\n[skin_xml_test]\nvendor=retained\n");
    let skin = Skin::load(repo().join("TrafficMonitor/skins/xml_test")).unwrap();
    let mut o = settings::options(&ini, Path::new(""), Some(&skin), false);
    o.font_name = "Live Skin Font".into();
    o.font_size = 17;
    o.font_style = 14;
    settings::save_options(&mut ini, &o);
    settings::save_skin_options(&mut ini, "xml_test", &o);
    let loaded = settings::options(&ini, Path::new(""), Some(&skin), false);
    assert_eq!(loaded.font_name, "Live Skin Font");
    assert_eq!(loaded.font_size, 17);
    assert_eq!(loaded.font_style, 14);
    assert_eq!(ini.get("config", "font_name"), Some("old-global"));
    assert_eq!(ini.get("skin_xml_test", "vendor"), Some("retained"));
}

#[test]
fn missing_taskbar_colors_default_to_white_and_units_are_untouched() {
    let mut ini = IniDocument::parse(
        "[task_bar]\nspecify_each_item_color=true\ntask_bar_text_color=1,2,\nunit_byte=false\ntask_bar_speed_unit=2\n[config]\nspeed_unit=1\n",
    );
    let o = settings::options(&ini, Path::new(""), None, false);
    assert_eq!(o.taskbar_style.item_colors[1].label, Some(0xffffff));
    settings::save_options(&mut ini, &o);
    assert_eq!(ini.get("task_bar", "unit_byte"), Some("false"));
    assert_eq!(ini.get("task_bar", "task_bar_speed_unit"), Some("2"));
    assert_eq!(ini.get("config", "speed_unit"), Some("1"));
}

#[test]
fn independent_theme_flags_and_font_bitmasks_survive_save() {
    for style in 0..16 {
        let mut ini = IniDocument::parse(
            "[skins]\nskin_auto_adapt=true\n[task_bar]\nauto_adapt_light_theme=false\n",
        );
        ini.set_i64("task_bar", "font_style", style);
        let o = settings::options(&ini, Path::new(""), None, false);
        assert_eq!(i64::from(o.taskbar_style.font_style), style);
        settings::save_options(&mut ini, &o);
        assert!(ini.get_bool("skins", "skin_auto_adapt", false));
        assert!(!ini.get_bool("task_bar", "auto_adapt_light_theme", true));
    }
}

#[test]
fn empty_history_and_calendar_boundaries_do_not_panic() {
    let (rows, summary) = stats::rows(
        &HistoryDocument::default(),
        "",
        "",
        HistoryGrouping::Day,
        true,
    )
    .unwrap();
    assert!(rows.is_empty());
    assert!(!summary.is_empty());
    let doc = HistoryDocument::new(vec![
        split(1900, 1, 1, 1, 1),
        split(3000, 12, 31, 1, 1),
        split(2000, 2, 29, 1, 1),
    ]);
    let (rows, _) = stats::rows(&doc, "", "", HistoryGrouping::Week, true).unwrap();
    assert!(rows.iter().any(|r| r.date.starts_with("1899/12/31")));
    assert!(rows.iter().any(|r| r.date.contains("3001/")));
    assert!(stats::parse_date("2000/02/29").is_ok());
}

#[test]
fn taskbar_theme_preset_is_applied_and_customization_saved_to_that_preset() {
    let mut ini = IniDocument::parse(
        "[task_bar]\nauto_adapt_light_theme=true\nlight_default_style=3\ndark_default_style=0\ngraph_color_following_system=false\n[taskbar_default_style]\ndefault4_text_color=11,12,13,14\ndefault4_back_color=1234\ndefault4_transparent_color=1234\ndefault4_status_bar_color=5678\ndefault4_specify_each_item_color=true\n",
    );
    let mut o = settings::options(&ini, Path::new(""), None, false);
    assert_eq!(o.taskbar_style.background, 1234);
    assert_eq!(o.taskbar_style.item_colors[1].label, Some(13));
    assert_eq!(o.taskbar_style.graph_color, Some(5678));
    o.taskbar_style.background = 1111;
    o.taskbar_style.item_colors[0].label = Some(777);
    settings::save_options(&mut ini, &o);
    assert_eq!(
        ini.get_i64("taskbar_default_style", "default4_back_color", 0),
        1111
    );
    assert_eq!(
        ini.get_i64_array("taskbar_default_style", "default4_text_color", 1, 0)[0],
        777
    );
    assert_eq!(
        settings::options(&ini, Path::new(""), None, true)
            .taskbar_style
            .background,
        0
    );
}

#[test]
fn automatic_graph_color_does_not_destroy_configured_manual_color() {
    let mut ini = IniDocument::parse(
        "[task_bar]\ngraph_color_following_system=true\nstatus_bar_color=12345\n",
    );
    let o = settings::options(&ini, Path::new(""), None, false);
    settings::save_options(&mut ini, &o);
    assert_eq!(ini.get_i64("task_bar", "status_bar_color", 0), 12345);
    assert!(ini.get_bool("task_bar", "graph_color_following_system", false));
}

#[test]
fn tray_auto_adaptation_uses_all_six_original_icon_resources() {
    for (index, name) in [
        "notifyicon.ico",
        "notifyicon2.ico",
        "notifyicon3.ico",
        "TrafficMonitor.ico",
        "notifyicon4.ico",
        "notifyicon5.ico",
    ]
    .iter()
    .enumerate()
    {
        let mut ini = IniDocument::default();
        ini.set_i64("config", "notify_icon_selected", index as i64);
        ini.set_bool("config", "notify_icon_auto_adapt", false);
        let o = settings::options(&ini, &repo().join("TrafficMonitor"), None, true);
        assert_eq!(
            o.tray_icon
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            *name
        );
    }
    let ini = IniDocument::parse("[config]\nnotify_icon_selected=0\nnotify_icon_auto_adapt=true\n");
    let o = settings::options(&ini, &repo().join("TrafficMonitor"), None, false);
    assert!(o.tray_icon.unwrap().ends_with("notifyicon4.ico"));
}

#[test]
fn english_history_summary_and_empty_state_use_selected_language() {
    let language =
        traffic_monitor_config::LanguageCatalog::load(repo().join("TrafficMonitor/language"))
            .unwrap()
            .select("en-US");
    let doc = HistoryDocument::new(vec![split(2024, 1, 1, 1, 2)]);
    let (_, summary) =
        stats::rows_localized(&doc, "", "", HistoryGrouping::Day, true, &language).unwrap();
    assert!(summary.contains("Upload"));
    assert!(summary.contains("Download"));
    assert!(
        !summary
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    );
    let (_, empty) = stats::rows_localized(
        &HistoryDocument::default(),
        "",
        "",
        HistoryGrouping::Day,
        true,
        &language,
    )
    .unwrap();
    assert_eq!(empty, "No history data");
}
