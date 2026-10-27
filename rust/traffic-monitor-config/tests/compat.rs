use std::{fs, path::PathBuf};
use traffic_monitor_config::*;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn temp() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "tm-config-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn ini_preserves_unknown_duplicates_and_comments() {
    let mut ini =
        IniDocument::parse("; retained\r\n[config]\r\nx = 1\r\nx=2\r\nunknown = vendor\r\n");
    assert_eq!(ini.get_i64("config", "x", 0), 1);
    ini.set_i64("config", "x", 3);
    let p = temp();
    ini.save_atomic(p.join("config.ini")).unwrap();
    let text = fs::read_to_string(p.join("config.ini")).unwrap();
    assert!(text.contains("; retained"));
    assert!(text.contains("x=2"));
    assert!(text.contains("unknown = vendor"));
    assert_eq!(
        IniDocument::load(p.join("config.ini"))
            .unwrap()
            .get_i64("config", "x", 0),
        3
    );
    fs::remove_dir_all(p).unwrap();
}

#[test]
fn ini_encodings_escaping_lists_arrays() {
    let source = "[配置]\n名=中文\n";
    let mut b = vec![0xff, 0xfe];
    for u in source.encode_utf16() {
        b.extend(u.to_le_bytes());
    }
    assert_eq!(
        IniDocument::parse_bytes(&b).unwrap().get("配置", "名"),
        Some("中文")
    );
    let mut ini = IniDocument::default();
    ini.set("s", "v", " quoted\n\\\" # ");
    ini.set_string_list("s", "list", &["a,b".into(), "\"你好\"".into(), "".into()]);
    ini.set_i64_array("s", "a", &[1, 2]);
    let round = IniDocument::parse(&ini.to_ini_string());
    assert_eq!(round.get("s", "v"), Some(" quoted\n\\\" # "));
    assert_eq!(
        round.get_string_list("s", "list"),
        vec!["a,b", "\"你好\"", ""]
    );
    assert_eq!(round.get_i64_array("s", "a", 4, 0), vec![1, 2, 2, 2]);
    assert!(IniDocument::parse_bytes(&[0xff, 0xfe, 0]).is_err());
}

#[test]
fn atomic_failure_retains_old_file_and_paths_follow_global_switch() {
    let p = temp();
    let exe = p.join("bin");
    let app = p.join("appdata");
    fs::create_dir_all(&exe).unwrap();
    let mut ini = IniDocument::default();
    ini.set_bool("config", "portable_mode", false);
    ini.save_atomic(exe.join("global_cfg.ini")).unwrap();
    let paths = AppPaths::resolve(exe.clone(), app.clone(), true).unwrap();
    assert!(!paths.portable);
    assert_eq!(paths.config_dir, app.join("TrafficMonitor"));
    assert!(!AppPaths::resolve(exe, app, false).unwrap().portable);
    fs::create_dir(p.join("blocked")).unwrap();
    fs::write(p.join("blocked/keep"), "original").unwrap();
    assert!(ini.save_atomic(p.join("blocked")).is_err());
    assert_eq!(
        fs::read_to_string(p.join("blocked/keep")).unwrap(),
        "original"
    );
    fs::remove_dir_all(p).unwrap();
}

#[test]
fn real_languages_skin_ini_and_xml_are_loaded() {
    let catalog = LanguageCatalog::load(repo().join("TrafficMonitor/language")).unwrap();
    assert!(catalog.languages.len() >= 18);
    let zh = catalog.select("zh-CN");
    assert_eq!(zh.info.bcp_47, "zh-CN");
    assert!(!zh.text("text", "IDS_MEMORY_USAGE", "").is_empty());
    let skins = enumerate_skins(repo().join("TrafficMonitor/skins")).unwrap();
    assert!(skins.len() >= 15);
    for entry in skins {
        let skin = Skin::load(&entry.path).unwrap();
        assert!(skin.small.width > 0);
        assert!(skin.large.height > 0);
        assert!(skin.background_small.is_some());
    }
    let xml = Skin::load(repo().join("TrafficMonitor/skins/xml_test")).unwrap();
    assert_eq!(xml.plugin_map.get("battery").unwrap(), "b5R30ITQ");
    assert_eq!(
        xml.large.items["battery"].plugin_id.as_deref(),
        Some("b5R30ITQ")
    );
    assert_eq!(xml.small.width, 232);
    assert_eq!(xml.labels["up"], "上传: ");
}

#[test]
fn formatting_and_settings_are_typed() {
    let f = FormatOptions::default();
    assert_eq!(f.speed(1024.0), "1.00 KB/s");
    let f = FormatOptions {
        unit_byte: false,
        ..f
    };
    assert_eq!(f.speed(128.0), "1.00 Kb/s");
    assert_eq!(format_parameters("<%2%> <%1%> <%1%>", &["a", "b"]), "b a a");
    let schema = settings_schema();
    assert!(schema.len() > 80);
    assert!(schema.iter().all(|s| !s.label.is_empty()));
    let mut ini = IniDocument::default();
    assert!(apply_setting(&mut ini, "general", "monitor_time_span", "1").is_err());
    apply_setting(&mut ini, "general", "monitor_time_span", "500").unwrap();
    assert_eq!(ini.get_i64("general", "monitor_time_span", 0), 500);
}

#[test]
fn language_legacy_identity_fallback_and_literal_parameters() {
    let catalog = LanguageCatalog::load(repo().join("TrafficMonitor/language")).unwrap();
    let language = catalog.select("es-ES|Español (España)|");
    assert_eq!(language.info.display_name, "Español (España)");
    assert_eq!(
        format_parameters("<%1%> <%2%>", &["<%2%>", "value"]),
        "<%2%> value"
    );
    assert_eq!(display_item_label("up", None, false), "↑: ");
    let short = FormatOptions {
        short: true,
        ..FormatOptions::default()
    };
    assert_eq!(short.speed(1024.0), "1.0 K/s");
    assert_eq!(
        DISPLAY_ITEMS
            .iter()
            .find(|i| i.id == "cpu_temperature")
            .unwrap()
            .ini_key,
        "cpu_temp_string"
    );
}

#[test]
fn xml_entities_and_legacy_skin_override_keys_are_resolved() {
    let mut skin=Skin::from_xml("<root><skin><skin_author>A &amp; B</skin_author></skin><plugin_map><clock>clock-id</clock></plugin_map><layout text_height=\"20\"><layout_l width=\"100\" height=\"40\"><clock x=\"2\" y=\"3\" width=\"90\" show=\"1\"/></layout_l><layout_s width=\"100\" height=\"20\"/></layout></root>").unwrap();
    assert_eq!(skin.author, "A & B");
    skin.name = "sample".into();
    skin.apply_overrides(&IniDocument::parse(
        "[skin_sample]\ncpu_temp_string=CPU熱\nclock-id=時刻\n",
    ));
    assert_eq!(skin.labels["cpu_temperature"], "CPU熱");
    assert_eq!(skin.labels["clock"], "時刻");
    let mut ini = IniDocument::default();
    skin.save_overrides(&mut ini);
    assert_eq!(ini.get("skin_sample", "cpu_temp_string"), Some("CPU熱"));
    assert_eq!(ini.get("skin_sample", "clock-id"), Some("時刻"));
    assert!(Skin::from_xml("<root><layout></root>").is_err());
}

#[test]
fn legacy_windows_paths_and_global_defaults_survive() {
    let mut ini = IniDocument::parse(
        "[config]\ndouble_click_exe=C:\\tools\\new\\app.exe\nrelative=.\\new\\tools\n",
    );
    assert_eq!(
        ini.get("config", "double_click_exe"),
        Some("C:\\tools\\new\\app.exe")
    );
    assert_eq!(ini.get("config", "relative"), Some(".\\new\\tools"));
    ini.set("config", "double_click_exe", "C:\\tools\\new\\app.exe");
    assert_eq!(
        IniDocument::parse(&ini.to_ini_string()).get("config", "double_click_exe"),
        Some("C:\\tools\\new\\app.exe")
    );
    let p = temp();
    fs::write(p.join("global_cfg.ini"), "[config]\n").unwrap();
    assert!(
        !AppPaths::resolve(p.clone(), p.join("appdata"), true)
            .unwrap()
            .portable
    );
    fs::remove_dir_all(p).unwrap();
}

#[test]
fn ini_typed_arrays_bom_endianness_and_float_defaults() {
    let text = "[s]\na=1,2,\nb=-1\nc=TRUE\n";
    let mut bytes = vec![0xfe, 0xff];
    for n in text.encode_utf16() {
        bytes.extend(n.to_be_bytes());
    }
    let ini = IniDocument::parse_bytes(&bytes).unwrap();
    assert_eq!(ini.get_i64_array("s", "a", 3, 9), vec![1, 2, 2]);
    assert_eq!(ini.get_bool_array("s", "b", 3), vec![true, true, true]);
    assert!(!ini.get_bool("s", "c", true));
    assert!(IniDocument::parse_bytes(&[0xff, 0xfe, 0x00, 0xd8]).is_err());
}

#[test]
fn corrupt_bom_is_not_reinterpreted_as_ansi() {
    assert!(IniDocument::parse_bytes(b"\xef\xbb\xbf[s]\nx=\xff").is_err());
}

#[test]
fn fonts_preserve_all_four_legacy_bits_and_combinations() {
    for style in 0..16u8 {
        let mut ini = IniDocument::default();
        FontInfo {
            name: "Test Font".into(),
            size: 11,
            style,
        }
        .save(&mut ini, "s");
        let loaded = FontInfo::load(&ini, "s", &FontInfo::default());
        assert_eq!(loaded.style, style);
        assert_eq!(
            ini.get_bool_array("s", "font_style", 4),
            (0..4).map(|i| style & (1 << i) != 0).collect::<Vec<_>>()
        );
    }
    let alternate = FontInfo::load(
        &IniDocument::parse("[s]\nfont_style=14\n"),
        "s",
        &FontInfo::default(),
    );
    assert_eq!(alternate.style, 14);
    assert_eq!(
        FontInfo::load(&IniDocument::default(), "s", &alternate).style,
        14
    );
}

#[test]
fn list_paths_doubled_quotes_and_all_bits_roundtrip() {
    let mut ini = IniDocument::parse("[s]\nlist=\"C:\\tools\\new\",\"a\"\"b\"\n");
    assert_eq!(
        ini.get_string_list("s", "list"),
        vec!["C:\\tools\\new", "a\"b"]
    );
    ini.set_string_list("s", "list", &["C:\\tools\\new".into(), "a\"b".into()]);
    assert_eq!(
        IniDocument::parse(&ini.to_ini_string()).get_string_list("s", "list"),
        vec!["C:\\tools\\new", "a\"b"]
    );
    let mut flags = vec![false; 64];
    flags[63] = true;
    ini.set_bool_array("s", "bits", &flags);
    assert_eq!(ini.get_bool_array("s", "bits", 64), flags);
}

#[cfg(windows)]
#[test]
fn locked_configuration_is_never_truncated_and_temp_is_removed() {
    use std::os::windows::fs::OpenOptionsExt;
    let p = temp();
    let config = p.join("config.ini");
    fs::write(&config, "original bytes").unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&config)
        .unwrap();
    let mut ini = IniDocument::default();
    ini.set("s", "key", "new");
    assert!(ini.save_atomic(&config).is_err());
    drop(locked);
    assert_eq!(fs::read_to_string(&config).unwrap(), "original bytes");
    assert_eq!(fs::read_dir(&p).unwrap().count(), 1);
    fs::remove_dir_all(p).unwrap();
}

#[cfg(windows)]
#[test]
fn real_windows_acp_roundtrip() {
    use windows_sys::Win32::Globalization::{CP_ACP, WideCharToMultiByte};
    let source = "[s]\nkey=中文 café\n";
    let wide = source.encode_utf16().collect::<Vec<_>>();
    let mut used = 0;
    let count = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            wide.as_ptr(),
            wide.len() as i32,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            &mut used,
        )
    };
    assert!(count > 0);
    let mut bytes = vec![0u8; count as usize];
    assert!(
        unsafe {
            WideCharToMultiByte(
                CP_ACP,
                0,
                wide.as_ptr(),
                wide.len() as i32,
                bytes.as_mut_ptr(),
                count,
                std::ptr::null(),
                &mut used,
            )
        } > 0
    );
    let parsed = IniDocument::parse_bytes(&bytes).unwrap();
    assert!(parsed.get("s", "key").is_some());
    if used == 0 {
        assert_eq!(parsed.get("s", "key"), Some("中文 café"));
    }
}

#[test]
fn normalization_preserves_vendor_fields_and_dynamic_defaults() {
    let mut ini = IniDocument::parse("[vendor]\nkey=secret\n[general]\nmonitor_time_span=1\n");
    let warnings = normalize_settings(&mut ini, "zh-CN", true);
    assert_eq!(warnings.len(), 1);
    assert_eq!(ini.get_i64("general", "monitor_time_span", 0), 1000);
    assert_eq!(ini.get_i64("general", "update_source", 0), 1);
    assert_eq!(ini.get("vendor", "key"), Some("secret"));
    let schema = settings_schema();
    for spec in schema {
        assert!(
            spec.validate(spec.default).is_ok(),
            "{}/{} invalid default",
            spec.section,
            spec.key
        );
    }
    assert!(ini.get("task_bar", "up_string").is_none());
}

#[test]
fn labels_match_legacy_localized_display_names() {
    let zh = LanguageCatalog::load(repo().join("TrafficMonitor/language"))
        .unwrap()
        .select("zh-CN");
    assert_eq!(display_item_label("gpu", Some(&zh), true), "显卡: ");
    assert_eq!(
        display_item_label("gpu_temperature", Some(&zh), true),
        "显卡: "
    );
    assert_eq!(
        display_item_label("hdd_temperature", Some(&zh), true),
        "硬盘: "
    );
}

#[test]
fn setting_labels_retain_intentional_surrounding_spaces() {
    let mut ini = IniDocument::default();
    apply_setting(&mut ini, "task_bar", "up_string", " ↑: ").unwrap();
    assert_eq!(ini.get("task_bar", "up_string"), Some(" ↑: "));
}

#[test]
fn plugin_string_cache_has_key_then_section_and_language_overrides() {
    let language = LanguageCatalog::load(repo().join("TrafficMonitor/language"))
        .unwrap()
        .select("zh-CN");
    let strings = language.strings();
    let key = ("IDS_MEMORY_USAGE".to_owned(), "text".to_owned());
    assert_eq!(
        strings.get(&key),
        Some(&language.text("text", "IDS_MEMORY_USAGE", ""))
    );
    assert!(!strings.contains_key(&("text".to_owned(), "IDS_MEMORY_USAGE".to_owned())));
}

#[test]
fn english_setting_schema_reuses_pack_labels_and_translates_choices() {
    let catalog = LanguageCatalog::load(repo().join("TrafficMonitor/language")).unwrap();
    let language = catalog.select("en-US");
    let fields = settings_schema_localized(&catalog, &[], &language);
    assert_eq!(fields.len(), 130);
    assert!(
        fields
            .iter()
            .any(|s| s.key == "auto_run_use_task_scheduler" && s.kind == SettingKind::Boolean)
    );
    let check = fields
        .iter()
        .find(|f| f.key == "check_update_when_start")
        .unwrap();
    assert_eq!(check.label, language.text("text", "TXT_CHECK_UPDATE", ""));
    assert!(fields.iter().all(|f| {
        !f.label
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    }));
    let memory = fields
        .iter()
        .find(|f| f.section == "config" && f.key == "memory_display")
        .unwrap();
    assert!(
        memory
            .options
            .iter()
            .any(|v| v.value == "2" && v.label == "Memory available")
    );
}

#[test]
fn formats_thresholds_unavailable_values_memory_and_order() {
    let f = FormatOptions::default();
    assert_eq!(f.speed(10240.0), "10.0 KB/s");
    assert_eq!(f.speed(1024000.0), "0.98 MB/s");
    assert_eq!(f.percentage(-1.0), "-- %");
    assert_eq!(f.temperature(0.0), "-- °C");
    assert_eq!(f.frequency(3.5), "3.50 GHz");
    let f = FormatOptions {
        memory_display: MemoryDisplay::Used,
        ..f
    };
    assert_eq!(f.memory(1073741824, 536870912), "512.00 MB");
    assert_eq!(f.graph_fraction(20.0, 10.0), 1.0);
    let order = display_order("2,0,2,77");
    assert_eq!(order[0], "cpu");
    assert_eq!(order[1], "up");
    assert_eq!(order.len(), 13);
}

#[test]
fn atomic_binary_payload_is_exact() {
    let p = temp();
    let file = p.join("history.dat");
    fs::write(&file, "old").unwrap();
    let bytes = [0u8, 255, 3, 4, 0, 128];
    save_bytes_atomic(&file, &bytes).unwrap();
    assert_eq!(fs::read(file).unwrap(), bytes);
    assert_eq!(fs::read_dir(&p).unwrap().count(), 1);
    fs::remove_dir_all(p).unwrap();
}

#[test]
fn skin_catalog_applies_theme_selection_and_overrides() {
    let catalog = SkinCatalog::load(repo().join("TrafficMonitor/skins")).unwrap();
    let mut ini = IniDocument::parse(
        "[config]\nskin_selected=.\\skins\\0默认皮肤\n[skins]\nskin_auto_adapt=true\nskin_name_light_mode=xml_test\n[skin_xml_test]\nfont_style=6\n",
    );
    assert_eq!(catalog.select(&ini, false).unwrap().name, "0默认皮肤");
    assert_eq!(catalog.select(&ini, true).unwrap().font.style, 6);
    ini.set("skins", "skin_name_light_mode", "missing");
    assert!(catalog.select(&ini, true).is_err());
}

#[test]
fn real_background_image_headers_are_loaded_and_validated() {
    let skin = Skin::load(repo().join("TrafficMonitor/skins/0默认皮肤")).unwrap();
    let image = skin.background(false).unwrap().unwrap();
    assert_eq!(image.format, ImageFormat::Png);
    assert!(image.width > 0);
    assert!(image.height > 0);
    assert!(!image.bytes.is_empty());
    let skin = Skin::load(repo().join("TrafficMonitor/skins/皮肤03")).unwrap();
    assert_eq!(
        skin.background(true).unwrap().unwrap().format,
        ImageFormat::Bmp
    );
    let p = temp();
    fs::write(p.join("bad.png"), b"broken").unwrap();
    assert!(ImageResource::load(p.join("bad.png")).is_err());
    fs::remove_dir_all(p).unwrap();
}
