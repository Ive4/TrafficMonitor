//! Production monitoring backend consumed by the independent Rust desktop.
use crate::{
    selection::{Interface, Mode, Selector},
    settings, stats,
    storage::HistoryStore,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::Write,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
use traffic_monitor_config::{
    self as config, AppPaths, DISPLAY_ITEMS, FormatOptions, IniDocument, Language, LanguageCatalog,
    Skin, SkinCatalog,
};
use traffic_monitor_desktop_ui::*;
use traffic_monitor_history::{Date, MergeMode};
use traffic_monitor_plugins::{self as plugins, HostInfo, MonitorInfo, PluginManager};
use traffic_monitor_windows::{
    advanced::{self, AdapterDetail, HardwareSampler},
    network, plugin_updates,
    sensors::SensorSampler,
    services, system,
};

#[derive(Clone, Default)]
pub struct LaunchOptions {
    pub data_dir: Option<PathBuf>,
    pub resource_dir: Option<PathBuf>,
    pub plugin_dir: Option<PathBuf>,
    pub exit_after_ms: Option<u32>,
    pub taskbar: bool,
    pub no_plugins: bool,
    pub test_mode: bool,
}
pub fn resolve_paths(launch: &LaunchOptions) -> Result<AppPaths, String> {
    let mut paths = if launch.data_dir.is_some() {
        let module = std::env::current_exe()
            .map_err(err)?
            .parent()
            .ok_or("程序目录不存在")?
            .to_owned();
        AppPaths::resolve(
            module,
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_default(),
            false,
        )
        .map_err(err)?
    } else {
        AppPaths::discover().map_err(err)?
    };
    if let Some(dir) = &launch.data_dir {
        paths.config_dir = dir.clone();
        paths.config_file = dir.join("config.ini");
        paths.history_file = dir.join("history_traffic.dat");
        paths.error_log = dir.join("error.log");
        paths.portable = true;
    }
    Ok(paths)
}
pub struct Runtime {
    paths: AppPaths,
    resources: PathBuf,
    ini: IniDocument,
    languages: LanguageCatalog,
    language: Language,
    skins: SkinCatalog,
    skin: Option<Skin>,
    options: DesktopOptions,
    selector: Selector,
    previous_cpu: system::CpuTimes,
    hardware: HardwareSampler,
    sensors: SensorSampler,
    details: Vec<AdapterDetail>,
    last_sample: Instant,
    last_details: Instant,
    last_flush: Instant,
    history: Option<HistoryStore>,
    range: (String, String, HistoryGrouping),
    plugins: PluginManager,
    host: HostInfo,
    plugin_order: Vec<String>,
    graphs: BTreeMap<String, VecDeque<f64>>,
    notices: Vec<String>,
    seen: BTreeSet<String>,
    thresholds: BTreeSet<String>,
    last_alert: BTreeMap<String, Instant>,
    update: Option<mpsc::Receiver<Vec<String>>>,
    test_mode: bool,
    closed: bool,
    auto_start_enabled: Option<bool>,
}
impl Runtime {
    pub fn load(launch: LaunchOptions) -> Result<Self, String> {
        let mut paths = resolve_paths(&launch)?;
        let resources = launch.resource_dir.unwrap_or_else(|| {
            if paths.module_dir.join("skins").is_dir() {
                paths.module_dir.clone()
            } else {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../TrafficMonitor")
            }
        });
        paths.skins_dir = resources.join("skins");
        paths.language_dir = resources.join("language");
        if let Some(dir) = launch.plugin_dir {
            paths.plugins_dir = dir;
        }
        paths.ensure_config_dir().map_err(err)?;
        let mut ini = match IniDocument::load(&paths.config_file) {
            Ok(i) => i,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => IniDocument::default(),
            Err(e) => return Err(format!("配置无法读取，原文件已保留：{e}")),
        };
        let languages = LanguageCatalog::load(&paths.language_dir).map_err(err)?;
        let language = languages.select(&ini.get_string("general", "language", ""));
        let dark = services::current_theme()
            .map(|t| t.system_dark)
            .unwrap_or(false);
        let mut notices = config::normalize_settings(&mut ini, &language.info.bcp_47, !dark);
        let skins = SkinCatalog::load(&paths.skins_dir).map_err(err)?;
        let skin = match skins.select(&ini, !dark) {
            Ok(s) => Some(s),
            Err(e) => {
                notices.push(format!("皮肤不可用，使用默认布局：{e}"));
                None
            }
        };
        let mut options = settings::options(&ini, &resources, skin.as_ref(), dark);
        options.exit_after_ms = launch.exit_after_ms;
        if launch.taskbar {
            options.show_taskbar = true;
        }
        let history = match HistoryStore::open(&paths.history_file, today()?) {
            Ok(mut s) => {
                notices.append(&mut s.notices);
                Some(s)
            }
            Err(e) => {
                notices.push(e.to_string());
                None
            }
        };
        let host = HostInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            config_dir: paths.plugin_config_dir(),
            strings: language.strings(),
            language_id: language.info.language_id() as u16,
            theme_color: services::current_theme().map(|t| t.color).unwrap_or(0),
            ..Default::default()
        };
        fs::create_dir_all(&host.config_dir).map_err(err)?;
        let mut plugins = PluginManager::new(host.clone());
        if !launch.no_plugins {
            plugins.discover(
                &paths.plugins_dir,
                &csv(&ini.get_string("config", "plugin_disabled", "")),
            )?;
        }
        let plugin_order = plugins
            .items()?
            .into_iter()
            .map(|i| i.id)
            .collect::<Vec<_>>();
        let details = advanced::network_details().map_err(err)?;
        let now = Instant::now();
        let range = (
            String::new(),
            String::new(),
            grouping(ini.get_i64("histroy_traffic", "view_type", 0)),
        );
        let mut app = Self {
            paths,
            resources,
            ini,
            languages,
            language,
            skins,
            skin,
            options,
            selector: Selector::default(),
            previous_cpu: system::cpu_times().map_err(err)?,
            hardware: HardwareSampler::new(),
            sensors: SensorSampler::new(),
            details,
            last_sample: now,
            last_details: now,
            last_flush: now,
            history,
            range,
            plugins,
            host,
            plugin_order,
            graphs: BTreeMap::new(),
            notices,
            seen: BTreeSet::new(),
            thresholds: BTreeSet::new(),
            last_alert: BTreeMap::new(),
            update: None,
            test_mode: launch.test_mode,
            closed: false,
            auto_start_enabled: None,
        };
        let exit_after = app.options.exit_after_ms;
        let force_taskbar = launch.taskbar;
        app.reload()?;
        app.options.exit_after_ms = exit_after;
        if force_taskbar {
            app.options.show_taskbar = true;
        }
        app.configure_selection();
        if !app.test_mode {
            app.refresh_auto_start();
        }
        if !app.test_mode && app.ini.get_bool("general", "check_update_when_start", true) {
            app.begin_update();
        }
        app.log(&format!(
            "start {} {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::ARCH
        ));
        Ok(app)
    }
    pub fn data_dir(&self) -> &std::path::Path {
        &self.paths.config_dir
    }
    fn refresh_auto_start(&mut self) {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let registry = services::auto_start_enabled(&exe, false);
        let scheduler = services::auto_start_enabled(&exe, true);
        match (registry, scheduler) {
            (Ok(registry), Ok(scheduler)) => {
                self.auto_start_enabled = Some(registry || scheduler);
                if scheduler {
                    self.ini
                        .set_bool("other", "auto_run_use_task_scheduler", true);
                }
            }
            (Err(e), _) | (_, Err(e)) => {
                self.auto_start_enabled = None;
                self.notice(format!("Auto-start state: {e}"));
            }
        }
    }
    fn configure_selection(&mut self) {
        let mode = if self.ini.get_bool("connection", "select_all", false) {
            Mode::All
        } else if self.ini.get_bool("connection", "auto_select", true) {
            Mode::Auto
        } else {
            Mode::Manual
        };
        let mut ids: BTreeSet<u64> = csv(&self.ini.get_string("connection", "selected_ids", ""))
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();
        if mode == Mode::Manual && ids.is_empty() {
            let name = self.ini.get_string("connection", "connection_name", "");
            ids.extend(
                self.details
                    .iter()
                    .filter(|d| {
                        !name.is_empty()
                            && (d.description == name || d.alias == name || d.adapter_name == name)
                    })
                    .map(|d| d.id),
            );
        }
        self.selector.configure(mode, ids);
    }
    fn read_network(
        &mut self,
        now: Instant,
        snapshot: std::io::Result<Vec<network::NetworkAdapter>>,
    ) -> Result<(Vec<network::NetworkAdapter>, crate::selection::Reading, u64), String> {
        let adapters = snapshot.map_err(err)?;
        let elapsed = now
            .duration_since(self.last_sample)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let hidden = self.ini.get_string_list("general", "connections_hide");
        let input: Vec<_> = adapters
            .iter()
            .filter(|a| !hidden.contains(&a.description) && !hidden.contains(&a.alias))
            .map(|a| {
                let detail = self.details.iter().find(|d| d.id == a.id);
                Interface {
                    id: a.id,
                    received: a.received_bytes,
                    sent: a.sent_bytes,
                    up: a.operational,
                    loopback: a.is_loopback,
                    eligible: detail.is_some(),
                    physical: detail.is_some_and(|d| d.physical),
                    gateway: detail.is_some_and(|d| !d.gateways.is_empty()),
                }
            })
            .collect();
        let reading = self.selector.update(&input, elapsed);
        self.last_sample = now;
        Ok((adapters, reading, elapsed))
    }
    fn reload(&mut self) -> Result<(), String> {
        self.language = self
            .languages
            .select(&self.ini.get_string("general", "language", ""));
        let dark = services::current_theme()
            .map(|t| t.system_dark)
            .unwrap_or(self.options.dark);
        self.skin = self.skins.select(&self.ini, !dark).ok();
        let mut options = settings::options_with_plugin_order(
            &self.ini,
            &self.resources,
            self.skin.as_ref(),
            dark,
            &self.plugin_order,
        );
        options.exit_after_ms = self.options.exit_after_ms;
        self.options = options;
        self.host.strings = self.language.strings();
        self.host.language_id = self.language.info.language_id() as u16;
        unsafe {
            windows_sys::Win32::Globalization::SetThreadUILanguage(self.host.language_id);
            windows_sys::Win32::Globalization::SetThreadLocale(u32::from(self.host.language_id));
        }
        self.range.2 = grouping(self.ini.get_i64("histroy_traffic", "view_type", 0));
        let mask = self.ini.get_i64("general", "hardware_monitor_item", 0);
        self.sensors
            .set_enabled(mask & 1 != 0, mask & 2 != 0, mask & 4 != 0, mask & 8 != 0);
        self.configure_selection();
        self.plugins.update_host(self.host.clone())
    }
    fn apply_fields(
        &self,
        ini: &mut IniDocument,
        batch: &[(String, String)],
    ) -> Result<(), String> {
        let schema =
            config::settings_schema_localized(&self.languages, &self.skins.entries, &self.language);
        for (key, value) in batch {
            let (s, k) = key.split_once('/').ok_or("Invalid setting identifier")?;
            let spec = schema
                .iter()
                .find(|spec| spec.section == s && spec.key == k)
                .ok_or_else(|| format!("Unknown setting: {key}"))?;
            let raw = spec
                .options
                .iter()
                .find(|o| o.label == *value)
                .map(|o| o.value.as_str())
                .unwrap_or(value);
            let value = spec.validate(raw)?;
            if s == "general" && k == "connections_hide" {
                if value.contains(['\r', '\n']) {
                    return Err("Connection names cannot contain line breaks".into());
                }
                let list = IniDocument::parse(&format!("[input]\nlist = {value}"))
                    .get_string_list("input", "list");
                ini.set_string_list(s, k, &list);
            } else {
                ini.set(s, k, value);
            }
        }
        Ok(())
    }
    fn commit_preferences(
        &mut self,
        ini: IniDocument,
        auto_start: Option<bool>,
    ) -> Result<(), String> {
        let previous = self.ini.clone();
        let old_method = previous.get_bool("other", "auto_run_use_task_scheduler", false);
        let new_method = ini.get_bool("other", "auto_run_use_task_scheduler", false);
        if auto_start.is_some() && self.auto_start_enabled.is_none() {
            return Err("Auto-start state is unavailable".into());
        }
        let operation = if let Some(enabled) =
            auto_start.filter(|enabled| self.auto_start_enabled != Some(*enabled))
        {
            Some((enabled, if enabled { new_method } else { old_method }))
        } else if old_method != new_method && self.auto_start_enabled == Some(true) {
            Some((true, new_method))
        } else {
            None
        };
        if operation.is_some() && self.test_mode {
            return Err("Validation mode cannot modify auto-start".into());
        }
        self.commit(ini)?;
        if let Some((enabled, method)) = operation {
            if let Err(error) =
                services::set_auto_start(&std::env::current_exe().map_err(err)?, enabled, method)
            {
                let restored = self.commit(previous);
                self.refresh_auto_start();
                return Err(format!(
                    "Auto-start change failed: {error}{}",
                    restored
                        .err()
                        .map(|e| format!("; configuration restore failed: {e}"))
                        .unwrap_or_default()
                ));
            }
            self.refresh_auto_start();
        }
        Ok(())
    }
    fn commit(&mut self, ini: IniDocument) -> Result<(), String> {
        let penetrating = !self.ini.get_bool("config", "mouse_penetrate", false)
            && ini.get_bool("config", "mouse_penetrate", false)
            && ini.get_bool("other", "show_mouse_panetrate_tip", true);
        ini.save_atomic(&self.paths.config_file).map_err(err)?;
        self.ini = ini;
        self.closed = false;
        self.log("configuration saved");
        if penetrating {
            self.notices.push(self.language.text("text","IDS_MOUSE_PENETRATE_TIP","Click-through enabled. Use the tray icon or taskbar menu to recover the floating monitor."));
        }
        self.reload()
    }
    fn log(&self, message: &str) {
        if self.ini.get_bool("other", "debug_log", false) {
            if let Ok(mut file) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.paths.error_log)
            {
                let _ = writeln!(file, "{:?} {message}", std::time::SystemTime::now());
            }
        }
    }
    fn notice(&mut self, message: String) {
        if self.seen.insert(message.clone()) {
            self.log(&message);
            self.notices.push(message);
        }
    }
    fn begin_update(&mut self) {
        if self.update.is_some() {
            return;
        }
        let (source, sender) = (
            self.ini.get_i64("general", "update_source", 0) as u32,
            mpsc::channel(),
        );
        self.update = Some(sender.1);
        let installed = self
            .plugins
            .plugins()
            .into_iter()
            .filter(|p| p.enabled && !p.properties[4].is_empty())
            .map(|p| (p.id, p.properties[4].clone()))
            .collect::<Vec<_>>();
        std::thread::spawn(move || {
            let mut messages = vec![
                match services::check_update(env!("CARGO_PKG_VERSION"), source) {
                    Ok(info) if info.available => {
                        format!("TrafficMonitor {}: {}", info.latest, info.url)
                    }
                    Ok(info) => format!("TrafficMonitor: {} (up to date)", info.latest),
                    Err(e) => format!("Update check: {e}"),
                },
            ];
            match plugin_updates::check_updates(source, &installed) {
                Ok(updates) => messages.extend(
                    updates
                        .into_iter()
                        .map(|u| format!("{}: {} → {}", u.file_name, u.current, u.latest)),
                ),
                Err(e) => messages.push(format!("Plugin updates: {e}")),
            }
            let _ = sender.0.send(messages);
        });
    }
    fn poll_update(&mut self) {
        if let Some(receiver) = &self.update {
            match receiver.try_recv() {
                Ok(messages) => {
                    self.update = None;
                    for message in messages {
                        self.log(&message);
                        self.notices.push(message);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.update = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }
    fn threshold(&mut self, key: &str, exceeded: bool, message: String) {
        let cooldown = Duration::from_secs(
            self.ini
                .get_i64("other", "notify_interval", 60)
                .clamp(1, 86400) as u64,
        );
        if exceeded {
            if self.thresholds.insert(key.into())
                && self
                    .last_alert
                    .get(key)
                    .is_none_or(|t| t.elapsed() >= cooldown)
            {
                self.last_alert.insert(key.into(), Instant::now());
                self.notices.push(message);
            }
        } else {
            self.thresholds.remove(key);
        }
    }
    fn settings_fields(&self) -> Vec<SettingField> {
        config::settings_schema_localized(&self.languages, &self.skins.entries, &self.language)
            .iter()
            .filter(|s| s.key != "show_dot_net_notinstalled_tip" && s.key != "last_light_mode")
            .map(|s| {
                let raw = if s.section == "general" && s.key == "connections_hide" {
                    csv_encode(&self.ini.get_string_list(s.section, s.key))
                } else {
                    s.value(&self.ini)
                };
                let value = if s.kind == config::SettingKind::Choice {
                    s.options
                        .iter()
                        .find(|o| o.value == raw)
                        .map(|o| o.label.clone())
                        .unwrap_or(raw)
                } else {
                    raw
                };
                let kind = match s.kind {
                    config::SettingKind::Boolean => SettingKind::Bool,
                    config::SettingKind::Integer => SettingKind::Integer,
                    config::SettingKind::Text => SettingKind::Text,
                    config::SettingKind::Choice => {
                        SettingKind::Choice(s.options.iter().map(|o| o.label.clone()).collect())
                    }
                    config::SettingKind::Color => SettingKind::Color,
                    config::SettingKind::Font => SettingKind::Font,
                    config::SettingKind::List => SettingKind::List,
                };
                SettingField {
                    key: format!("{}/{}", s.section, s.key),
                    label: s.label.clone(),
                    value,
                    kind,
                    help: match (s.min, s.max) {
                        (Some(a), Some(b)) => format!("{a}–{b}"),
                        _ => String::new(),
                    },
                }
            })
            .collect()
    }
    fn host_formats(&mut self, main: &[DisplayValue], taskbar: &[DisplayValue]) {
        for (host_index, id) in [
            "up",
            "down",
            "cpu",
            "memory",
            "gpu",
            "cpu_temperature",
            "gpu_temperature",
            "hdd_temperature",
            "main_board_temperature",
            "hdd",
            "cpu_freq",
        ]
        .iter()
        .enumerate()
        {
            if let Some(v) = main.iter().find(|v| v.id == *id) {
                self.host.main_value_strings[host_index] = v.value.clone();
            }
            if let Some(v) = taskbar.iter().find(|v| v.id == *id) {
                self.host.taskbar_value_strings[host_index] = v.value.clone();
            }
        }
        let directions = self
            .history
            .as_ref()
            .and_then(HistoryStore::today_directions);
        for (index, value) in [(11, directions.map(|p| p.0)), (12, directions.map(|p| p.1))] {
            let text = value
                .map(|v| config::format_bytes(v as f64, true))
                .unwrap_or_else(|| "--".into());
            self.host.main_value_strings[index] = text.clone();
            self.host.taskbar_value_strings[index] = text;
        }
        let keys = [
            ("config", "speed_short_mode"),
            ("config", "separate_value_unit_with_space"),
            ("config", "unit_byte"),
            ("config", "speed_unit"),
            ("config", "hide_unit"),
            ("config", "hide_percent"),
            ("task_bar", "task_bar_speed_short_mode"),
            ("task_bar", "separate_value_unit_with_space"),
            ("task_bar", "value_right_align"),
            ("task_bar", "digits_number"),
            ("task_bar", "unit_byte"),
            ("task_bar", "task_bar_speed_unit"),
            ("task_bar", "task_bar_hide_unit"),
            ("task_bar", "task_bar_hide_percent"),
        ];
        for (index, (s, k)) in keys.iter().enumerate() {
            let raw = self.ini.get_string(s, k, "0");
            let value = match raw.as_str() {
                "true" => "1".into(),
                "false" => "0".into(),
                _ => raw,
            };
            self.host.extended.insert(index as i32 + 3, value);
        }
    }
}
impl DesktopBackend for Runtime {
    fn options(&self) -> DesktopOptions {
        self.options.clone()
    }
    fn sample(&mut self) -> Result<Dashboard, String> {
        self.poll_update();
        let now = Instant::now();
        if let Ok(theme) = services::current_theme() {
            self.host.theme_color = theme.color;
            if self
                .ini
                .get_bool("other", "last_light_mode", !theme.system_dark)
                != !theme.system_dark
            {
                self.ini
                    .set_bool("other", "last_light_mode", !theme.system_dark);
                self.reload()?;
                self.closed = false;
            }
        }
        if now.duration_since(self.last_details) > Duration::from_secs(5) {
            match advanced::network_details() {
                Ok(v) => self.details = v,
                Err(e) => self.notice(format!("网络详情读取失败：{e}")),
            }
            self.last_details = now;
        }
        let network_snapshot = network::snapshot();
        let (adapters, reading, elapsed) = self.read_network(Instant::now(), network_snapshot)?;
        let cpu = match system::cpu_times() {
            Ok(current) => {
                let usage = system::cpu_usage(self.previous_cpu, current);
                self.previous_cpu = current;
                usage
            }
            Err(e) => {
                self.previous_cpu = system::CpuTimes::default();
                self.notice(format!("CPU: {e}"));
                None
            }
        };
        let memory = match system::memory_status() {
            Ok(memory) => Some(memory),
            Err(e) => {
                self.notice(format!("Memory: {e}"));
                None
            }
        };
        let hardware = self
            .hardware
            .sample(&self.ini.get_string("general", "hard_disk_name", ""));
        let sensors = self.sensors.sample(
            &self
                .ini
                .get_string("general", "cpu_core_name", "Core Average"),
            &self.ini.get_string("general", "hard_disk_name", ""),
        );
        for message in hardware.diagnostics.iter().chain(&sensors.diagnostics) {
            if message.contains("PDH warming up:") {
                self.log(message);
            } else {
                self.notice(message.clone());
            }
        }
        let cpu = if self.ini.get_i64("general", "cpu_usage_acquire_method", 1) == 1 {
            hardware.cpu_usage_percent.or(cpu)
        } else {
            cpu
        };
        let mut history_error = None;
        let mut history_notices = Vec::new();
        if let Some(history) = &mut self.history {
            if let Err(e) = history.add(today()?, reading.received_delta, reading.sent_delta) {
                history_error = Some(e.to_string());
            }
            if now.duration_since(self.last_flush) > Duration::from_secs(15) {
                if let Err(e) = history.flush() {
                    history_error = Some(format!("历史记录保存失败：{e}"));
                }
                self.last_flush = now;
            }
            history_notices.append(&mut history.notices);
        }
        for note in history_notices {
            self.notice(note);
        }
        if let Some(e) = history_error {
            self.notice(e);
        }
        let today_bytes = self
            .history
            .as_ref()
            .map(|h| h.total_today_bytes())
            .unwrap_or(0);
        let up = reading.sent_per_second as f64;
        let down = reading.received_per_second as f64;
        let mut values = [-1.; 13];
        values[0] = up;
        values[1] = down;
        values[2] = cpu.unwrap_or(-1.);
        values[3] = memory.map(|m| f64::from(m.load_percent)).unwrap_or(-1.);
        values[4] = hardware.gpu_usage_percent.unwrap_or(-1.);
        values[5] = sensors.cpu_temperature.unwrap_or(-1.);
        values[6] = sensors.gpu_temperature.unwrap_or(-1.);
        values[7] = sensors.hdd_temperature.unwrap_or(-1.);
        values[8] = sensors.mainboard_temperature.unwrap_or(-1.);
        values[9] = hardware.disk_usage_percent.unwrap_or(-1.);
        values[10] = up + down;
        values[11] = hardware.cpu_frequency_mhz.map(|f| f / 1000.).unwrap_or(-1.);
        values[12] = if self.history.is_some() {
            today_bytes as f64
        } else {
            -1.
        };
        self.host.monitor_values = monitor_values(
            values,
            self.history
                .as_ref()
                .and_then(HistoryStore::today_directions),
        );
        for info in DISPLAY_ITEMS {
            let graph = self.graphs.entry(info.id.into()).or_default();
            graph.push_back(values[usize::from(info.index)].max(0.));
            if graph.len() > 60 {
                graph.pop_front();
            }
        }
        let render = |taskbar: bool| {
            let format = FormatOptions::from_ini(&self.ini, taskbar);
            DISPLAY_ITEMS
                .iter()
                .map(|info| {
                    let n = values[usize::from(info.index)];
                    let value = match info.index {
                        0 | 1 | 10 => format.speed(n),
                        3 => memory
                            .map(|m| format.memory(m.total_bytes, m.available_bytes))
                            .unwrap_or_else(|| "--".into()),
                        5..=8 => format.temperature(n),
                        11 => format.frequency(n),
                        12 => config::format_bytes(n, true),
                        _ => format.percentage(n),
                    };
                    let label = if taskbar {
                        self.ini.get("task_bar", info.ini_key).map(str::to_owned)
                    } else {
                        self.skin
                            .as_ref()
                            .and_then(|s| {
                                s.labels.get(info.id).or_else(|| s.labels.get(info.ini_key))
                            })
                            .cloned()
                    }
                    .unwrap_or_else(|| {
                        config::display_item_label(info.id, Some(&self.language), !taskbar)
                    });
                    let maximum = if matches!(info.index, 0 | 1 | 10) {
                        self.options
                            .taskbar_style
                            .graph_maximum
                            .unwrap_or(10. * 1024. * 1024.)
                    } else if info.index == 11 {
                        6.
                    } else {
                        100.
                    };
                    DisplayValue {
                        id: info.id.into(),
                        label,
                        value,
                        fraction: (n >= 0.).then_some((n / maximum).clamp(0., 1.)),
                        history: self
                            .graphs
                            .get(info.id)
                            .map(|v| v.iter().copied().collect())
                            .unwrap_or_default(),
                        color: None,
                        plugin: false,
                        ..Default::default()
                    }
                })
                .collect::<Vec<_>>()
        };
        let mut items = render(false);
        let mut taskbar_items = render(true);
        self.host_formats(&items, &taskbar_items);
        self.plugins.update_host(self.host.clone())?;
        self.plugins.refresh(MonitorInfo {
            up_speed: reading.sent_per_second,
            down_speed: reading.received_per_second,
            cpu_usage: values[2] as i32,
            memory_usage: values[3] as i32,
            gpu_usage: values[4] as i32,
            hdd_usage: values[9] as i32,
            cpu_temperature: values[5] as i32,
            gpu_temperature: values[6] as i32,
            hdd_temperature: values[7] as i32,
            main_board_temperature: values[8] as i32,
            cpu_freq: values[11] as i32,
        })?;
        for item in self.plugins.items()? {
            let display = DisplayValue {
                id: item.id,
                label: item.label,
                value: item.value,
                fraction: item.graph.map(f64::from),
                history: Vec::new(),
                color: None,
                plugin: true,
                custom_draw: item.custom_draw,
                width: Some(item.width),
                double_line: item.double_line,
            };
            items.push(display.clone());
            taskbar_items.push(display);
        }
        let plugins = self
            .plugins
            .plugins()
            .into_iter()
            .map(|p| PluginInfo {
                id: p.id,
                name: p.properties[0].clone(),
                version: p.properties[4].clone(),
                enabled: p.enabled,
                description: p.error.unwrap_or_else(|| p.properties[1].clone()),
                commands: p
                    .commands
                    .into_iter()
                    .map(|c| (c.index as u32, c.name))
                    .collect(),
            })
            .collect();
        let mut tooltip = format!(
            "↑ {}  ↓ {}\nCPU {}  RAM {}",
            items[0].value, items[1].value, items[2].value, items[3].value
        );
        for plugin in self.plugins.plugins().into_iter().filter(|p| p.enabled) {
            if let Ok(text) = self.plugins.tooltip(&plugin.id) {
                if !text.is_empty() {
                    tooltip.push('\n');
                    tooltip.push_str(&text);
                }
            }
        }
        for note in self.plugins.take_notifications() {
            self.notice(note);
        }
        let hidden = self.ini.get_string_list("general", "connections_hide");
        let networks = adapters
            .iter()
            .filter(|a| {
                self.ini.get_bool("general", "show_all_interface", false)
                    || self.details.iter().any(|d| d.id == a.id)
            })
            .filter(|a| !hidden.contains(&a.description) && !hidden.contains(&a.alias))
            .map(|a| {
                let mut details = vec![
                    ("Description".into(), a.description.clone()),
                    ("Interface LUID".into(), a.id.to_string()),
                    (
                        "State".into(),
                        if a.operational { "Up" } else { "Down" }.into(),
                    ),
                    (
                        "Received".into(),
                        config::format_bytes(a.received_bytes as f64, true),
                    ),
                    (
                        "Sent".into(),
                        config::format_bytes(a.sent_bytes as f64, true),
                    ),
                ];
                if let Some(d) = self.details.iter().find(|d| d.id == a.id) {
                    details.extend([
                        ("IP".into(), d.addresses.join("; ")),
                        ("Gateway".into(), d.gateways.join("; ")),
                        ("DNS".into(), d.dns.join("; ")),
                        ("MAC".into(), d.mac.clone()),
                        ("MTU".into(), d.mtu.to_string()),
                        (
                            "Link speed".into(),
                            format!("{} Mbps", d.link_speed_bps / 1_000_000),
                        ),
                    ]);
                }
                NetworkInfo {
                    id: a.id.to_string(),
                    name: a.alias.clone(),
                    selected: reading.selected.contains(&a.id),
                    details,
                }
            })
            .collect();
        let (history, history_summary) = if let Some(h) = &mut self.history {
            stats::rows_localized(
                h.document(),
                &self.range.0,
                &self.range.1,
                self.range.2,
                self.ini.get_bool("histroy_traffic", "sunday_first", true),
                &self.language,
            )?
        } else {
            (Vec::new(), "历史文件不可用，原文件已保留".into())
        };
        let memory_exceeded = self
            .ini
            .get_bool("notify_tip", "memory_usage_tip_enable", false)
            && values[3] >= self.ini.get_i64("notify_tip", "memory_tip_value", 80) as f64;
        self.threshold(
            "memory",
            memory_exceeded,
            format!(
                "{} {}%",
                self.language
                    .text("text", "IDS_MEMORY_UDAGE_EXCEED", "Memory usage reached"),
                values[3]
            ),
        );
        let traffic_limit = self
            .ini
            .get_i64("notify_tip", "traffic_tip_value", 200)
            .max(1) as u128
            * if self.ini.get_i64("notify_tip", "traffic_tip_unit", 0) == 0 {
                1024 * 1024
            } else {
                1024 * 1024 * 1024
            };
        self.threshold(
            "traffic",
            self.ini.get_bool("notify_tip", "traffic_tip_enable", false)
                && today_bytes >= traffic_limit,
            format!(
                "今日流量已达到 {}",
                config::format_bytes(today_bytes as f64, true)
            ),
        );
        for (key, index, label) in [
            ("cpu", 5, "CPU"),
            ("gpu", 6, "GPU"),
            ("hdd", 7, "HDD"),
            ("mainboard", 8, "Mainboard"),
        ] {
            let enabled = self.ini.get_bool(
                "notify_tip",
                &format!("{key}_temperature_tip_enable"),
                false,
            );
            let limit = self
                .ini
                .get_i64("notify_tip", &format!("{key}_temperature_tip_value"), 80)
                as f64;
            self.threshold(
                key,
                enabled && values[index] >= limit,
                format!("{label}: {:.0} °C", values[index]),
            );
        }
        Ok(Dashboard {
            current_skin: self
                .skin
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_default(),
            current_language: if self.ini.get_string("general", "language", "").is_empty() {
                String::new()
            } else {
                self.language.info.config_string()
            },
            connection_mode: if self.ini.get_bool("connection", "select_all", false) {
                ConnectionMode::All
            } else if self.ini.get_bool("connection", "auto_select", true) {
                ConnectionMode::Auto
            } else {
                ConnectionMode::Manual
            },
            history_grouping: self.range.2,
            history_from: self.range.0.clone(),
            history_to: self.range.1.clone(),
            auto_start_enabled: self.auto_start_enabled,
            auto_start_scheduler: self
                .ini
                .get_bool("other", "auto_run_use_task_scheduler", false),
            items,
            taskbar_items,
            tooltip,
            status: format!(
                "{} network(s) | {} ms | Rust",
                reading.selected.len(),
                elapsed
            ),
            networks,
            history,
            history_summary,
            plugins,
            settings: self.settings_fields(),
            skins: self
                .skins
                .entries
                .iter()
                .map(|s| Choice {
                    id: s.name.clone(),
                    name: s.name.clone(),
                })
                .collect(),
            languages: std::iter::once(Choice {
                id: String::new(),
                name: self
                    .language
                    .text("text", "RUST_FOLLOW_SYSTEM", "Follow system"),
            })
            .chain(self.languages.languages.iter().map(|l| Choice {
                id: l.config_string(),
                name: l.display_name.clone(),
            }))
            .collect(),
            notices: std::mem::take(&mut self.notices),
        })
    }
    fn dispatch(&mut self, action: DesktopAction) -> Result<(), String> {
        match action {
            DesktopAction::ApplyPreferences {
                options,
                fields,
                skin,
                language,
                auto_start,
            } => {
                if !(200..=30000).contains(&options.interval_ms)
                    || !(5..=72).contains(&options.font_size)
                    || !(5..=72).contains(&options.taskbar_style.font_size)
                    || !(1..=4096).contains(&options.taskbar_width)
                    || !(3..=12).contains(&options.taskbar_style.value_digits)
                {
                    return Err(
                        "Display or sampling settings are outside the supported range".into(),
                    );
                }
                let mut ini = self.ini.clone();
                settings::save_options(&mut ini, &options);
                if let Some(current) = &self.skin {
                    settings::save_skin_options_with_plugin_order(
                        &mut ini,
                        &current.name,
                        &options,
                        &self.plugin_order,
                    );
                }
                self.apply_fields(&mut ini, &fields)?;
                if let Some(name) = skin {
                    if !self.skins.entries.iter().any(|s| s.name == name) {
                        return Err("Skin is unavailable".into());
                    }
                    ini.set("config", "skin_selected", name.clone());
                    if ini.get_bool("skins", "skin_auto_adapt", false) {
                        ini.set(
                            "skins",
                            if self.options.dark {
                                "skin_name_dark_mode"
                            } else {
                                "skin_name_light_mode"
                            },
                            name,
                        );
                    }
                }
                if let Some(id) = language {
                    if !id.is_empty()
                        && !self
                            .languages
                            .languages
                            .iter()
                            .any(|l| l.config_string() == id || l.bcp_47 == id)
                    {
                        return Err("Language is unavailable".into());
                    }
                    ini.set("general", "language", id);
                }
                self.commit_preferences(ini, auto_start)?;
            }
            DesktopAction::ApplyOptions(o) => {
                let mut ini = self.ini.clone();
                settings::save_options(&mut ini, &o);
                if let Some(s) = &self.skin {
                    settings::save_skin_options_with_plugin_order(
                        &mut ini,
                        &s.name,
                        &o,
                        &self.plugin_order,
                    );
                }
                self.commit(ini)?;
            }
            DesktopAction::SaveSettings(batch) => {
                let mut ini = self.ini.clone();
                self.apply_fields(&mut ini, &batch)?;
                self.commit_preferences(ini, None)?;
            }
            DesktopAction::SelectConnections { mode, ids } => {
                let ids = ids
                    .iter()
                    .map(|id| id.parse::<u64>().map(|v| v.to_string()).map_err(err))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut ini = self.ini.clone();
                ini.set_bool("connection", "auto_select", mode == ConnectionMode::Auto);
                ini.set_bool("connection", "select_all", mode == ConnectionMode::All);
                ini.set("connection", "selected_ids", ids.join(","));
                self.commit(ini)?;
            }
            DesktopAction::HistoryRange { from, to, grouping } => {
                if !from.is_empty() {
                    stats::parse_date(&from)?;
                }
                if !to.is_empty() {
                    stats::parse_date(&to)?;
                }
                if !from.is_empty()
                    && !to.is_empty()
                    && stats::parse_date(&from)? > stats::parse_date(&to)?
                {
                    return Err("开始日期不能晚于结束日期".into());
                }
                let mut ini = self.ini.clone();
                ini.set_i64("histroy_traffic", "view_type", grouping_value(grouping));
                self.commit(ini)?;
                self.range = (from, to, grouping);
            }
            DesktopAction::ImportHistory(path) => {
                self.history
                    .as_mut()
                    .ok_or("历史主文件损坏，请先恢复有效备份，原文件未改动")?
                    .import(&path, MergeMode::Accumulate)
                    .map_err(err)?;
            }
            DesktopAction::ExportHistory(path) => {
                self.history
                    .as_ref()
                    .ok_or("没有可导出的有效历史文件")?
                    .export(&path)
                    .map_err(err)?;
            }
            DesktopAction::SelectSkin(name) => {
                if !self.skins.entries.iter().any(|s| s.name == name) {
                    return Err("皮肤不存在".into());
                }
                let mut ini = self.ini.clone();
                ini.set("config", "skin_selected", name);
                if ini.get_bool("skins", "skin_auto_adapt", false) {
                    ini.set(
                        "skins",
                        if self.options.dark {
                            "skin_name_dark_mode"
                        } else {
                            "skin_name_light_mode"
                        },
                        ini.get_string("config", "skin_selected", ""),
                    );
                }
                self.commit(ini)?;
            }
            DesktopAction::SelectLanguage(id) => {
                if !id.is_empty()
                    && !self
                        .languages
                        .languages
                        .iter()
                        .any(|l| l.config_string() == id || l.bcp_47 == id)
                {
                    return Err("语言不存在".into());
                }
                let mut ini = self.ini.clone();
                ini.set("general", "language", id);
                self.commit(ini)?;
            }
            DesktopAction::PluginEnabled { id, enabled } => {
                let mut disabled = csv(&self.ini.get_string("config", "plugin_disabled", ""));
                disabled.retain(|i| !i.eq_ignore_ascii_case(&id));
                if !enabled {
                    disabled.push(id.clone());
                }
                self.plugins.set_enabled(&id, enabled)?;
                self.plugin_order = self.plugins.items()?.into_iter().map(|i| i.id).collect();
                let mut ini = self.ini.clone();
                ini.set("config", "plugin_disabled", disabled.join(","));
                self.commit(ini)?;
            }
            DesktopAction::PluginConfigure(id) => {
                self.plugins.configure(&id, self.host.main_window)?;
            }
            DesktopAction::PluginCommand { id, command } => {
                self.plugins
                    .command(&id, command as usize, self.host.main_window)?
            }
            DesktopAction::SetAutoStart(enabled) => {
                if self.test_mode {
                    return Err("测试模式不修改系统启动项".into());
                }
                services::set_auto_start(
                    &std::env::current_exe().map_err(err)?,
                    enabled,
                    self.ini
                        .get_bool("other", "auto_run_use_task_scheduler", false),
                )
                .map_err(err)?;
                self.refresh_auto_start();
            }
            DesktopAction::CheckUpdates => self.begin_update(),
            DesktopAction::OpenTaskManager => services::open_task_manager().map_err(err)?,
            DesktopAction::OpenDataDirectory => {
                services::open_path(&self.paths.config_dir).map_err(err)?
            }
            DesktopAction::OpenSpecificApp { taskbar } => {
                let path = self.ini.get_string(
                    if taskbar { "task_bar" } else { "config" },
                    "double_click_exe",
                    "",
                );
                if path.is_empty() {
                    return Err("请先在选项中指定双击打开的应用".into());
                }
                services::launch_application(std::path::Path::new(&path)).map_err(err)?;
            }
            DesktopAction::ToggleMore => {
                let mut ini = self.ini.clone();
                ini.set_bool(
                    "config",
                    "show_cpu_memory",
                    !ini.get_bool("config", "show_cpu_memory", false),
                );
                self.commit(ini)?;
            }
            DesktopAction::CycleSkin => {
                if !self.skins.entries.is_empty() {
                    let current = self
                        .skin
                        .as_ref()
                        .and_then(|s| self.skins.entries.iter().position(|v| v.name == s.name))
                        .unwrap_or(0);
                    let next = self.skins.entries[(current + 1) % self.skins.entries.len()]
                        .name
                        .clone();
                    self.dispatch(DesktopAction::SelectSkin(next))?;
                }
            }
            DesktopAction::ResetPosition => {
                let mut ini = self.ini.clone();
                ini.set_i64("config", "position_x", 40);
                ini.set_i64("config", "position_y", 40);
                self.commit(ini)?;
            }
            DesktopAction::PositionChanged { x, y } => {
                self.ini.set_i64("config", "position_x", i64::from(x));
                self.ini.set_i64("config", "position_y", i64::from(y));
                self.options.x = x;
                self.options.y = y;
                self.closed = false;
            }
        }
        Ok(())
    }
    fn text(&self, key: &str, fallback: &str) -> String {
        let (section, key) = key.split_once('/').unwrap_or(("text", key));
        self.language.text(section, key, fallback)
    }
    fn locale(&self) -> String {
        self.language.info.bcp_47.clone()
    }
    fn checkpoint(&mut self) -> Result<(), String> {
        if let Some(history) = &mut self.history {
            history.flush().map_err(err)?;
        }
        self.ini.save_atomic(&self.paths.config_file).map_err(err)?;
        self.log("session checkpoint saved");
        Ok(())
    }
    fn attach_windows(&mut self, main: usize, taskbar: Option<usize>) {
        self.host.main_window = main;
        self.host.taskbar_window = taskbar.unwrap_or(0);
        self.host.main_dpi = window_dpi(main);
        self.host.taskbar_dpi = taskbar.map(window_dpi).unwrap_or(self.host.main_dpi);
        if let Err(e) = self.plugins.update_host(self.host.clone()) {
            self.notice(e);
        }
    }
    fn draw_plugin(
        &mut self,
        id: &str,
        hdc: usize,
        rect: [i32; 4],
        dark: bool,
        taskbar: bool,
    ) -> bool {
        let (label, value) = if taskbar {
            let style = &self.options.taskbar_style;
            let colors = style.item_colors.iter().find(|i| i.id == id);
            (
                colors
                    .and_then(|c| c.label)
                    .or(style.label_color)
                    .unwrap_or(style.foreground),
                colors
                    .and_then(|c| c.value)
                    .or(style.value_color)
                    .unwrap_or(style.foreground),
            )
        } else {
            let color = self
                .options
                .skin
                .as_ref()
                .and_then(|s| s.items.iter().find(|i| i.id == id))
                .and_then(|i| i.color)
                .unwrap_or(self.options.foreground);
            (color, color)
        };
        if let Err(e) = self.plugins.set_draw_colors(id, label, value) {
            self.notice(e);
            return false;
        }
        match self.plugins.draw(
            id,
            hdc,
            [rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]],
            dark,
            taskbar,
        ) {
            Ok(v) => v,
            Err(e) => {
                self.notice(e);
                false
            }
        }
    }
    fn plugin_width(&mut self, id: &str, hdc: usize, taskbar: bool) -> Option<i32> {
        self.plugins.item_width(id, hdc, taskbar).ok()
    }
    fn plugin_mouse(&mut self, id: &str, event: PluginMouseEvent) -> bool {
        let kind = match event.kind {
            0 => plugins::MouseEventKind::Left,
            1 => plugins::MouseEventKind::Right,
            2 => plugins::MouseEventKind::Double,
            3 => plugins::MouseEventKind::WheelUp,
            4 => plugins::MouseEventKind::WheelDown,
            _ => return false,
        };
        self.plugins
            .mouse(
                id,
                plugins::MouseEvent {
                    kind,
                    x: event.x,
                    y: event.y,
                    window: event.window,
                    taskbar: event.taskbar,
                },
            )
            .unwrap_or(false)
    }
    fn plugin_keyboard(&mut self, id: &str, e: PluginKeyboardEvent) -> bool {
        self.plugins
            .keyboard(
                id,
                plugins::KeyboardEvent {
                    key: e.key,
                    ctrl: e.ctrl,
                    shift: e.shift,
                    alt: e.alt,
                    window: e.window,
                    taskbar: e.taskbar,
                },
            )
            .unwrap_or(false)
    }
    fn shutdown(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        let mut errors = Vec::new();
        if let Err(e) = self.sensors.shutdown() {
            errors.push(e);
        }
        if let Some(h) = &mut self.history {
            if let Err(e) = h.flush() {
                errors.push(e.to_string());
            }
        }
        if let Err(e) = self.ini.save_atomic(&self.paths.config_file) {
            errors.push(e.to_string());
        }
        if let Err(e) = self.plugins.shutdown() {
            errors.push(e);
        }
        if errors.is_empty() {
            self.log("shutdown complete");
            self.closed = true;
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if !self.closed {
            if let Err(e) = self.shutdown() {
                self.log(&format!("shutdown failed: {e}"));
            }
        }
    }
}
fn window_dpi(window: usize) -> i32 {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    let user: Vec<u16> = "user32.dll".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let module = GetModuleHandleW(user.as_ptr());
        if let Some(proc) = GetProcAddress(module, c"GetDpiForWindow".as_ptr().cast()) {
            let get: unsafe extern "system" fn(windows_sys::Win32::Foundation::HWND) -> u32 =
                std::mem::transmute(proc);
            return get(window as _).max(96) as i32;
        }
    }
    96
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
fn csv_encode(values: &[String]) -> String {
    values
        .iter()
        .map(|value| {
            if value.contains([',', '"']) {
                format!("\"{}\"", value.replace('"', "\"\""))
            } else {
                value.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}
fn grouping(v: i64) -> HistoryGrouping {
    match v {
        1 => HistoryGrouping::Week,
        2 => HistoryGrouping::Month,
        3 => HistoryGrouping::Quarter,
        4 => HistoryGrouping::Year,
        _ => HistoryGrouping::Day,
    }
}
fn grouping_value(v: HistoryGrouping) -> i64 {
    match v {
        HistoryGrouping::Day => 0,
        HistoryGrouping::Week => 1,
        HistoryGrouping::Month => 2,
        HistoryGrouping::Quarter => 3,
        HistoryGrouping::Year => 4,
    }
}
fn monitor_values(display: [f64; 13], directions: Option<(u128, u128)>) -> [f64; 13] {
    let mut result = display;
    result[10] = display[11];
    result[11] = directions.map(|d| d.0 as f64).unwrap_or(-1.);
    result[12] = directions.map(|d| d.1 as f64).unwrap_or(-1.);
    result
}
fn today() -> Result<Date, String> {
    let mut t = windows_sys::Win32::Foundation::SYSTEMTIME::default();
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
    Date::new(t.wYear, t.wMonth as u8, t.wDay as u8).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    #[test]
    fn plugin_monitor_order_has_frequency_and_separate_direction_totals() {
        let mut values = [0.; 13];
        values[10] = 999.;
        values[11] = 2.25;
        values[12] = 3072.;
        let host = monitor_values(values, Some((1024, 2048)));
        assert_eq!(&host[10..], &[2.25, 1024., 2048.]);
        assert_eq!(&monitor_values(values, None)[10..], &[2.25, -1., -1.]);
    }
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/native-app-tests")
                .join(format!(
                    "{}-{}",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn launch(&self) -> LaunchOptions {
            LaunchOptions {
                data_dir: Some(self.0.clone()),
                resource_dir: Some(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../TrafficMonitor"),
                ),
                no_plugins: true,
                test_mode: true,
                ..Default::default()
            }
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            if let Ok(p) = self.0.canonicalize() {
                let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/native-app-tests")
                    .canonicalize()
                    .unwrap();
                assert!(p.starts_with(parent));
                let _ = fs::remove_dir_all(p);
            }
        }
    }
    #[test]
    fn network_failure_keeps_interval_and_hidden_connections_are_not_counted() {
        let t = Temp::new();
        let mut app = Runtime::load(t.launch()).unwrap();
        app.selector.configure(Mode::Manual, BTreeSet::from([42]));
        let start = Instant::now();
        app.last_sample = start;
        let adapter = |received| network::NetworkAdapter {
            id: 42,
            interface_index: 1,
            alias: "Ethernet, Work".into(),
            description: "Test".into(),
            received_bytes: received,
            sent_bytes: 100,
            operational: true,
            is_loopback: false,
        };
        app.read_network(start, Ok(vec![adapter(1000)])).unwrap();
        assert!(
            app.read_network(
                start + Duration::from_secs(1),
                Err(std::io::Error::other("injected network failure"))
            )
            .is_err()
        );
        let (_, reading, elapsed) = app
            .read_network(start + Duration::from_secs(2), Ok(vec![adapter(3000)]))
            .unwrap();
        assert_eq!(elapsed, 2000);
        assert_eq!(reading.received_per_second, 1000);
        app.dispatch(DesktopAction::SaveSettings(vec![(
            "general/connections_hide".into(),
            "\"Ethernet, Work\",Wi-Fi".into(),
        )]))
        .unwrap();
        assert_eq!(
            app.ini.get_string_list("general", "connections_hide"),
            vec!["Ethernet, Work", "Wi-Fi"]
        );
        let (_, hidden, _) = app
            .read_network(start + Duration::from_secs(3), Ok(vec![adapter(5000)]))
            .unwrap();
        assert_eq!(hidden.received_delta, 0);
        assert!(!hidden.selected.contains(&42));
    }
    #[test]
    fn checkpoint_persists_without_stopping_the_runtime() {
        let t = Temp::new();
        let mut app = Runtime::load(t.launch()).unwrap();
        app.dispatch(DesktopAction::PositionChanged { x: 444, y: 333 })
            .unwrap();
        app.checkpoint().unwrap();
        assert!(app.sample().is_ok());
        let saved = IniDocument::load(t.0.join("config.ini")).unwrap();
        assert_eq!(saved.get_i64("config", "position_x", 0), 444);
        app.shutdown().unwrap();
    }
    #[test]
    #[ignore = "Explicit repository-built trusted PluginDemo; requires its matching MSVC runtime"]
    fn existing_cpp_plugin_runs_through_the_real_rust_backend() {
        use windows_sys::Win32::Graphics::Gdi::*;
        let t = Temp::new();
        let mut launch = t.launch();
        launch.no_plugins = false;
        launch.plugin_dir = Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            if cfg!(target_arch = "x86") {
                "../../Bin/Release/plugins"
            } else {
                "../../Bin/x64/Release/plugins"
            },
        ));
        let mut app = Runtime::load(launch).unwrap();
        let frame = app.sample().unwrap();
        let plugin = frame
            .plugins
            .iter()
            .find(|p| p.id == "PluginDemo.dll")
            .expect("build the original PluginDemo fixture before this explicit test");
        assert!(plugin.enabled);
        let custom = frame
            .items
            .iter()
            .filter(|i| i.plugin && i.custom_draw)
            .collect::<Vec<_>>();
        assert_eq!(custom.len(), 2);
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            let dc = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, 240, 80);
            ReleaseDC(std::ptr::null_mut(), screen);
            assert!(!dc.is_null() && !bitmap.is_null());
            let old = SelectObject(dc, bitmap);
            PatBlt(dc, 0, 0, 240, 80, BLACKNESS);
            for item in custom {
                assert!(
                    app.plugin_width(&item.id, dc as usize, true)
                        .is_some_and(|w| w > 0)
                );
                assert!(app.draw_plugin(&item.id, dc as usize, [0, 0, 220, 60], true, true));
            }
            SelectObject(dc, old);
            DeleteObject(bitmap);
            DeleteDC(dc);
        }
        app.dispatch(DesktopAction::PluginEnabled {
            id: "PluginDemo.dll".into(),
            enabled: false,
        })
        .unwrap();
        assert!(
            !app.sample()
                .unwrap()
                .plugins
                .iter()
                .find(|p| p.id == "PluginDemo.dll")
                .unwrap()
                .enabled
        );
        app.dispatch(DesktopAction::PluginEnabled {
            id: "PluginDemo.dll".into(),
            enabled: true,
        })
        .unwrap();
        assert_eq!(
            app.sample()
                .unwrap()
                .items
                .iter()
                .filter(|i| i.plugin)
                .count(),
            4
        );
        app.shutdown().unwrap();
    }
    #[test]
    fn combined_preferences_validate_every_tab_before_committing() {
        let t = Temp::new();
        let mut app = Runtime::load(t.launch()).unwrap();
        app.checkpoint().unwrap();
        let before = fs::read(t.0.join("config.ini")).unwrap();
        let mut options = app.options();
        let old_interval = options.interval_ms;
        options.interval_ms = 2000;
        let action = |fields| DesktopAction::ApplyPreferences {
            options: options.clone(),
            fields,
            skin: None,
            language: None,
            auto_start: None,
        };
        assert!(
            app.dispatch(action(vec![(
                "task_bar/status_bar_color".into(),
                "bad".into()
            )]))
            .is_err()
        );
        assert_eq!(app.options().interval_ms, old_interval);
        assert_eq!(fs::read(t.0.join("config.ini")).unwrap(), before);
        app.dispatch(action(vec![
            ("task_bar/status_bar_color".into(), "64".into()),
            ("notify_tip/memory_tip_value".into(), "91".into()),
        ]))
        .unwrap();
        assert_eq!(app.options().interval_ms, 2000);
        let saved = IniDocument::load(t.0.join("config.ini")).unwrap();
        assert_eq!(saved.get_i64("task_bar", "status_bar_color", 0), 64);
        assert_eq!(saved.get_i64("notify_tip", "memory_tip_value", 0), 91);
    }
    #[test]
    fn runtime_reads_real_data_and_flushes_its_isolated_history() {
        let t = Temp::new();
        let mut app = Runtime::load(t.launch()).unwrap();
        let frame = app.sample().unwrap();
        assert!(frame.notices.iter().all(|n| !n.contains("PDH warming up:")));
        assert!(!frame.networks.is_empty());
        assert!(
            frame
                .items
                .iter()
                .any(|i| i.id == "memory" && !i.value.is_empty())
        );
        app.shutdown().unwrap();
        assert!(t.0.join("config.ini").is_file());
        assert!(t.0.join("history_traffic.dat").is_file());
    }
    #[test]
    fn invalid_settings_batch_keeps_memory_and_config_unchanged() {
        let t = Temp::new();
        let mut app = Runtime::load(t.launch()).unwrap();
        app.shutdown().unwrap();
        let before = fs::read(t.0.join("config.ini")).unwrap();
        let interval = app.options().interval_ms;
        let result = app.dispatch(DesktopAction::SaveSettings(vec![
            ("general/monitor_time_span".into(), "2000".into()),
            ("config/transparency".into(), "not a number".into()),
        ]));
        assert!(result.is_err());
        assert_eq!(app.options().interval_ms, interval);
        assert_eq!(fs::read(t.0.join("config.ini")).unwrap(), before);
    }
    #[test]
    fn connection_and_position_actions_are_persisted() {
        let t = Temp::new();
        let mut app = Runtime::load(t.launch()).unwrap();
        app.dispatch(DesktopAction::SelectConnections {
            mode: ConnectionMode::Manual,
            ids: vec!["42".into()],
        })
        .unwrap();
        app.dispatch(DesktopAction::PositionChanged { x: 123, y: 456 })
            .unwrap();
        app.shutdown().unwrap();
        let ini = traffic_monitor_config::IniDocument::load(t.0.join("config.ini")).unwrap();
        assert!(!ini.get_bool("connection", "auto_select", true));
        assert_eq!(ini.get_i64("config", "position_x", 0), 123);
        assert_eq!(ini.get_i64("config", "position_y", 0), 456);
        assert!(
            ini.get_string("connection", "selected_ids", "")
                .contains("42")
        );
    }
    #[test]
    fn applying_options_updates_live_style_and_reopened_configuration() {
        let t = Temp::new();
        let mut app = Runtime::load(t.launch()).unwrap();
        let mut o = app.options();
        o.show_main = false;
        o.interval_ms = 2000;
        o.taskbar_style.show_graph = false;
        o.taskbar_style.font_size = 18;
        app.dispatch(DesktopAction::ApplyOptions(o)).unwrap();
        assert!(!app.options().show_main);
        assert_eq!(app.options().interval_ms, 2000);
        assert!(!app.options().taskbar_style.show_graph);
        app.shutdown().unwrap();
        let reopened = Runtime::load(t.launch()).unwrap();
        assert!(!reopened.options().show_main);
        assert_eq!(reopened.options().taskbar_style.font_size, 18);
    }
    #[test]
    fn corrupt_history_is_reported_and_never_overwritten() {
        let t = Temp::new();
        let bytes = b"unknown legacy data";
        fs::write(t.0.join("history_traffic.dat"), bytes).unwrap();
        let mut app = Runtime::load(t.launch()).unwrap();
        let frame = app.sample().unwrap();
        assert!(!frame.notices.is_empty());
        assert_eq!(
            frame
                .items
                .iter()
                .find(|i| i.id == "today_traffic")
                .unwrap()
                .value,
            "--"
        );
        app.shutdown().unwrap();
        assert_eq!(fs::read(t.0.join("history_traffic.dat")).unwrap(), bytes);
    }
}
