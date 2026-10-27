//! Real, optional temperature sources. Collection runs outside the UI thread.
//! No ACPI thermal-zone reading is presented as a CPU or motherboard sensor.
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default)]
pub struct SensorSnapshot {
    pub cpu_temperature: Option<f64>,
    pub gpu_temperature: Option<f64>,
    pub hdd_temperature: Option<f64>,
    pub mainboard_temperature: Option<f64>,
    pub cpu_cores: Vec<(String, f64)>,
    pub disks: Vec<(String, f64)>,
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Request {
    cpu: String,
    disk: String,
    enabled: [bool; 4],
}
struct Stamped {
    snapshot: SensorSnapshot,
    at: Instant,
}
/// Nonblocking UI-facing sampler. COM interfaces and DLL handles stay on its worker.
pub struct SensorSampler {
    requests: Option<mpsc::SyncSender<Request>>,
    results: mpsc::Receiver<Stamped>,
    last: Option<Stamped>,
    previous: Option<Request>,
    last_request: Option<Instant>,
    enabled: [bool; 4],
    stop: Arc<AtomicBool>,
    worker_id: Arc<AtomicU32>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Default for SensorSampler {
    fn default() -> Self {
        Self::new()
    }
}
impl SensorSampler {
    pub fn new() -> Self {
        let (tx, requests) = mpsc::sync_channel::<Request>(1);
        let (results, rx) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_id = Arc::new(AtomicU32::new(0));
        let worker_id_for_thread = worker_id.clone();
        let worker = std::thread::Builder::new()
            .name("TrafficMonitor sensors".into())
            .spawn(move || {
                #[cfg(windows)]
                worker_id_for_thread.store(
                    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
                    Ordering::Release,
                );
                #[cfg(not(windows))]
                let _ = worker_id_for_thread;
                if worker_stop.load(Ordering::Relaxed) {
                    return;
                }
                #[cfg(windows)]
                let mut engine = native::Engine::new();
                while !worker_stop.load(Ordering::Relaxed) {
                    let request = match requests.recv_timeout(Duration::from_millis(200)) {
                        Ok(r) => r,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    if worker_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    #[cfg(windows)]
                    let snapshot = match &mut engine {
                        Ok(engine) => engine.sample(&request, &worker_stop),
                        Err(error) => SensorSnapshot {
                            diagnostics: vec![error.clone()],
                            ..Default::default()
                        },
                    };
                    #[cfg(not(windows))]
                    let snapshot = {
                        let _ = request;
                        SensorSnapshot {
                            diagnostics: vec!["Hardware sensors require Windows".into()],
                            ..Default::default()
                        }
                    };
                    if results
                        .try_send(Stamped {
                            snapshot,
                            at: Instant::now(),
                        })
                        .is_err()
                        && worker_stop.load(Ordering::Relaxed)
                    {
                        break;
                    }
                }
            });
        let (worker, last) = match worker {
            Ok(worker) => (Some(worker), None),
            Err(error) => (
                None,
                Some(Stamped {
                    snapshot: SensorSnapshot {
                        diagnostics: vec![format!("Sensor worker could not start: {error}")],
                        ..Default::default()
                    },
                    at: Instant::now(),
                }),
            ),
        };
        Self {
            requests: Some(tx),
            results: rx,
            last,
            previous: None,
            last_request: None,
            enabled: [true; 4],
            stop,
            worker_id,
            worker,
        }
    }
    pub fn set_enabled(&mut self, cpu: bool, gpu: bool, disk: bool, mainboard: bool) {
        self.enabled = [cpu, gpu, disk, mainboard];
    }
    /// Stop collection, cancel pending synchronous COM calls, and join cleanup.
    /// A nonresponsive vendor DLL/provider is reported after three seconds; its
    /// worker keeps owning native resources until the call actually returns.
    pub fn shutdown(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Relaxed);
        self.requests.take();
        let started = Instant::now();
        while let Some(worker) = &self.worker {
            if worker.is_finished() {
                break;
            }
            #[cfg(windows)]
            {
                let id = self.worker_id.load(Ordering::Acquire);
                if id != 0 {
                    let _ = unsafe { windows::Win32::System::Com::CoCancelCall(id, 0) };
                }
            }
            if started.elapsed() >= Duration::from_secs(3) {
                return Err("sensor worker did not finish after COM cancellation; native resources remain owned by its in-flight call".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| "sensor worker panicked during shutdown".to_string())?;
        }
        Ok(())
    }
    pub fn sample(&mut self, cpu_core_name: &str, disk_name: &str) -> SensorSnapshot {
        while let Ok(sample) = self.results.try_recv() {
            self.last = Some(sample);
        }
        let request = Request {
            cpu: cpu_core_name.into(),
            disk: disk_name.into(),
            enabled: self.enabled,
        };
        let needs_request = self.previous.as_ref() != Some(&request)
            || self
                .last_request
                .is_none_or(|last| last.elapsed() >= Duration::from_secs(2));
        if needs_request
            && self
                .requests
                .as_ref()
                .is_some_and(|tx| tx.try_send(request.clone()).is_ok())
        {
            self.previous = Some(request);
            self.last_request = Some(Instant::now());
        }
        let mut result = match &self.last {
            Some(last) if last.at.elapsed() < Duration::from_secs(10) => last.snapshot.clone(),
            Some(_) => SensorSnapshot {
                diagnostics: vec![
                    "Sensor data expired; waiting for a current provider sample".into(),
                ],
                ..Default::default()
            },
            None => SensorSnapshot {
                diagnostics: vec!["Sensor discovery is running in the background".into()],
                ..Default::default()
            },
        };
        result.cpu_temperature = selected(&result.cpu_cores, cpu_core_name, true);
        result.hdd_temperature = selected(&result.disks, disk_name, false);
        if !self.enabled[0] {
            result.cpu_temperature = None;
            result.cpu_cores.clear();
        }
        if !self.enabled[1] {
            result.gpu_temperature = None;
        }
        if !self.enabled[2] {
            result.hdd_temperature = None;
            result.disks.clear();
        }
        if !self.enabled[3] {
            result.mainboard_temperature = None;
        }
        if self.enabled.iter().all(|&enabled| !enabled) {
            return SensorSnapshot::default();
        }
        result
    }
}
impl Drop for SensorSampler {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Cpu,
    GpuNvidia,
    GpuAmd,
    GpuIntel,
    Disk,
    Board,
}
#[derive(Clone, Debug)]
struct Reading {
    id: String,
    device_id: String,
    device: String,
    name: String,
    kind: Kind,
    value: f64,
}
#[derive(Clone, Debug)]
struct Hardware {
    id: String,
    parent: String,
    name: String,
    kind: Option<Kind>,
}
fn hardware_kind(kind: &str) -> Option<Kind> {
    match kind.to_ascii_lowercase().as_str() {
        "cpu" => Some(Kind::Cpu),
        "gpunvidia" => Some(Kind::GpuNvidia),
        "gpuamd" | "gpuati" => Some(Kind::GpuAmd),
        "gpuintel" => Some(Kind::GpuIntel),
        "storage" | "hdd" => Some(Kind::Disk),
        "motherboard" | "mainboard" => Some(Kind::Board),
        _ => None,
    }
}
fn classify<'a>(hardware: &'a BTreeMap<String, Hardware>, parent: &str) -> Option<&'a Hardware> {
    let mut parent = parent;
    for _ in 0..32 {
        let h = hardware.get(parent)?;
        if h.kind.is_some() {
            return Some(h);
        }
        if h.parent == parent {
            return None;
        }
        parent = &h.parent;
    }
    None
}
fn temperature(value: f64) -> Option<f64> {
    if value.is_finite() && value > 0. && value <= 200. {
        Some(value)
    } else {
        None
    }
}
fn named_values(readings: &[Reading], kind: Kind) -> Vec<(String, f64)> {
    if kind == Kind::Disk {
        let mut devices = BTreeMap::<&str, (&str, Vec<f64>)>::new();
        for reading in readings.iter().filter(|reading| reading.kind == kind) {
            devices
                .entry(&reading.device_id)
                .or_insert_with(|| (&reading.device, Vec::new()))
                .1
                .push(reading.value);
        }
        let mut values = BTreeMap::new();
        for (name, temperatures) in devices.into_values() {
            let mut unique = name.to_owned();
            let mut suffix = 1;
            while values.contains_key(&unique) {
                unique = format!("{name} #{suffix}");
                suffix += 1;
            }
            values.insert(
                unique,
                temperatures.iter().sum::<f64>() / temperatures.len() as f64,
            );
        }
        return values.into_iter().collect();
    }
    let mut values = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    for reading in readings.iter().filter(|r| r.kind == kind) {
        if !seen.insert(&reading.id) {
            continue;
        }
        let base = if kind == Kind::Disk {
            &reading.device
        } else {
            &reading.name
        };
        let mut name = base.clone();
        let mut suffix = 1;
        while values.contains_key(&name) {
            name = format!("{base} #{suffix}");
            suffix += 1;
        }
        values.insert(name, reading.value);
    }
    values.into_iter().collect()
}
fn selected(values: &[(String, f64)], name: &str, average: bool) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    if average
        && (name.is_empty()
            || name.eq_ignore_ascii_case("average")
            || name.eq_ignore_ascii_case("average temperature")
            || name.contains("平均"))
    {
        return Some(values.iter().map(|(_, v)| v).sum::<f64>() / values.len() as f64);
    }
    values
        .iter()
        .find(|(n, _)| n == name)
        .or_else(|| values.first())
        .map(|(_, v)| *v)
}
fn grouped_temperature(readings: &[Reading], kind: Kind, preferred_name: &str) -> Option<f64> {
    let first = readings
        .iter()
        .filter(|r| r.kind == kind)
        .min_by_key(|r| &r.device_id)?;
    let group = readings
        .iter()
        .filter(|r| r.kind == kind && r.device_id == first.device_id)
        .collect::<Vec<_>>();
    group
        .iter()
        .find(|r| r.name == preferred_name)
        .map(|r| r.value)
        .or_else(|| Some(group.iter().map(|r| r.value).sum::<f64>() / group.len() as f64))
}
fn snapshot(
    readings: &[Reading],
    request: &Request,
    mut diagnostics: Vec<String>,
) -> SensorSnapshot {
    let cpu_cores = if request.enabled[0] {
        named_values(readings, Kind::Cpu)
    } else {
        Vec::new()
    };
    let disks = if request.enabled[2] {
        named_values(readings, Kind::Disk)
    } else {
        Vec::new()
    };
    let cpu_temperature = selected(&cpu_cores, &request.cpu, true);
    let hdd_temperature = selected(&disks, &request.disk, false);
    if !request.cpu.is_empty()
        && !request.cpu.to_ascii_lowercase().starts_with("average")
        && !request.cpu.contains("平均")
        && !cpu_cores.is_empty()
        && !cpu_cores.iter().any(|(name, _)| name == &request.cpu)
    {
        diagnostics.push(format!(
            "CPU sensor '{}' unavailable; selected first named sensor",
            request.cpu
        ));
    }
    if !request.disk.is_empty()
        && !disks.is_empty()
        && !disks.iter().any(|(name, _)| name == &request.disk)
    {
        diagnostics.push(format!(
            "Disk sensor '{}' unavailable; selected first named disk",
            request.disk
        ));
    }
    let gpu_temperature = if request.enabled[1] {
        [Kind::GpuNvidia, Kind::GpuAmd, Kind::GpuIntel]
            .into_iter()
            .find_map(|kind| grouped_temperature(readings, kind, "GPU Core"))
    } else {
        None
    };
    let mainboard_temperature = if request.enabled[3] {
        grouped_temperature(readings, Kind::Board, "")
    } else {
        None
    };
    for (enabled, value, name) in [
        (request.enabled[0], cpu_temperature, "CPU"),
        (request.enabled[1], gpu_temperature, "GPU"),
        (request.enabled[2], hdd_temperature, "disk"),
        (request.enabled[3], mainboard_temperature, "motherboard"),
    ] {
        if enabled && value.is_none() {
            diagnostics.push(format!("No current {name} temperature sensor is available"));
        }
    }
    SensorSnapshot {
        cpu_temperature,
        gpu_temperature,
        hdd_temperature,
        mainboard_temperature,
        cpu_cores,
        disks,
        diagnostics,
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::ffi::{CStr, c_void};
    use std::ptr;
    use windows::Win32::System::{Com::*, Variant::*, Wmi::*};
    use windows::core::{BSTR, PCWSTR};
    use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
        LoadLibraryExW,
    };
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                let _ = CoDisableCallCancellation(None);
                CoUninitialize();
            }
        }
    }
    struct Value(VARIANT);
    impl Drop for Value {
        fn drop(&mut self) {
            let _ = unsafe { VariantClear(&mut self.0) };
        }
    }
    impl Value {
        fn property(object: &IWbemClassObject, name: &str) -> Result<Self, String> {
            let mut value = Self(VARIANT::default());
            let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            unsafe { object.Get(PCWSTR(name.as_ptr()), 0, &mut value.0, None, None) }
                .map_err(|e| format!("WMI property: {e}"))?;
            Ok(value)
        }
        fn text(&self) -> Result<String, String> {
            unsafe {
                let value = &self.0.Anonymous.Anonymous;
                if value.vt == VT_NULL || value.vt == VT_EMPTY {
                    return Ok(String::new());
                }
                if value.vt == VT_BSTR {
                    return Ok(value.Anonymous.bstrVal.to_string());
                }
                if matches!(
                    value.vt,
                    VT_UI1 | VT_UI2 | VT_UI4 | VT_I1 | VT_I2 | VT_I4 | VT_UI8 | VT_I8
                ) {
                    return VariantToDouble(&self.0)
                        .map(|n| format!("{n:.0}"))
                        .map_err(|e| e.to_string());
                }
                Err(format!("WMI field is not text ({})", value.vt.0))
            }
        }
        fn number(&self) -> Result<Option<f64>, String> {
            unsafe {
                let vt = self.0.Anonymous.Anonymous.vt;
                if vt == VT_NULL || vt == VT_EMPTY {
                    return Ok(None);
                }
                VariantToDouble(&self.0)
                    .map(Some)
                    .map_err(|e| e.to_string())
            }
        }
    }
    fn text(object: &IWbemClassObject, name: &str) -> Result<String, String> {
        Value::property(object, name)?.text()
    }
    fn number(object: &IWbemClassObject, name: &str) -> Result<Option<f64>, String> {
        Value::property(object, name)?.number()
    }
    struct Provider {
        namespace: &'static str,
        service: Option<IWbemServices>,
        retry: Option<Instant>,
        error: Option<String>,
    }
    impl Provider {
        fn new(namespace: &'static str) -> Self {
            Self {
                namespace,
                service: None,
                retry: None,
                error: None,
            }
        }
        fn query(
            &mut self,
            query: &str,
            stop: &AtomicBool,
        ) -> Result<Vec<IWbemClassObject>, String> {
            if stop.load(Ordering::Relaxed) {
                return Err("sensor worker stopping".into());
            }
            if self.retry.is_some_and(|retry| Instant::now() < retry) {
                return Err(self
                    .error
                    .clone()
                    .unwrap_or_else(|| "WMI provider retry deferred".into()));
            }
            let result = self.query_inner(query, stop);
            if let Err(error) = &result {
                let error = format!("{}: {error}", self.namespace);
                self.error = Some(error.clone());
                self.retry = Some(Instant::now() + Duration::from_secs(30));
                self.service = None;
                return Err(error);
            }
            self.error = None;
            self.retry = None;
            result
        }
        fn query_inner(
            &mut self,
            query: &str,
            stop: &AtomicBool,
        ) -> Result<Vec<IWbemClassObject>, String> {
            unsafe {
                if self.service.is_none() {
                    let locator: IWbemLocator =
                        CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)
                            .map_err(|e| e.to_string())?;
                    let empty = BSTR::new();
                    let service = locator
                        .ConnectServer(
                            &BSTR::from(self.namespace),
                            &empty,
                            &empty,
                            &empty,
                            128,
                            &empty,
                            None,
                        )
                        .map_err(|e| e.to_string())?;
                    CoSetProxyBlanket(
                        &service,
                        10,
                        0,
                        PCWSTR::null(),
                        RPC_C_AUTHN_LEVEL_CALL,
                        RPC_C_IMP_LEVEL_IMPERSONATE,
                        None,
                        EOAC_NONE,
                    )
                    .map_err(|e| e.to_string())?;
                    self.service = Some(service);
                }
                let service = self
                    .service
                    .as_ref()
                    .ok_or_else(|| "WMI connection unavailable".to_string())?;
                let enumeration = service
                    .ExecQuery(
                        &BSTR::from("WQL"),
                        &BSTR::from(query),
                        WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                        None,
                    )
                    .map_err(|e| e.to_string())?;
                let started = Instant::now();
                let mut result = Vec::new();
                loop {
                    if stop.load(Ordering::Relaxed) {
                        return Err("sensor worker stopping".into());
                    }
                    if started.elapsed() > Duration::from_secs(2) {
                        return Err("WMI enumeration exceeded 2 seconds".into());
                    }
                    let mut objects: [Option<IWbemClassObject>; 16] = std::array::from_fn(|_| None);
                    let mut count = 0;
                    let status = enumeration.Next(100, &mut objects, &mut count);
                    if status.is_err() {
                        return Err(format!("WMI enumeration: {status:?}"));
                    }
                    if count as usize > objects.len() {
                        return Err("WMI returned an invalid object count".into());
                    }
                    for object in objects.into_iter().take(count as usize) {
                        result.push(object.ok_or_else(|| "WMI returned null object".to_string())?);
                    }
                    if result.len() > 4096 {
                        return Err("WMI sensor limit exceeded".into());
                    }
                    if status.0 == 1 {
                        return Ok(result);
                    }
                    if count == 0 && status.0 == 0 {
                        return Ok(result);
                    }
                }
            }
        }
    }
    pub(super) struct Engine {
        lhm: Provider,
        ohm: Provider,
        storage: Provider,
        nvml: Option<Nvml>,
        nvml_error: Option<String>,
        nvml_retry: Option<Instant>,
        _com: Apartment,
    }
    impl Engine {
        pub(super) fn new() -> Result<Self, String> {
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED)
                    .ok()
                    .map_err(|e| format!("Sensor COM initialization: {e}"))?;
            }
            if let Err(error) = unsafe { CoEnableCallCancellation(None) } {
                unsafe {
                    CoUninitialize();
                }
                return Err(format!("Sensor COM cancellation initialization: {error}"));
            }
            Ok(Self {
                lhm: Provider::new("ROOT\\LibreHardwareMonitor"),
                ohm: Provider::new("ROOT\\OpenHardwareMonitor"),
                storage: Provider::new("ROOT\\Microsoft\\Windows\\Storage"),
                nvml: None,
                nvml_error: None,
                nvml_retry: None,
                _com: Apartment,
            })
        }
        pub(super) fn sample(&mut self, request: &Request, stop: &AtomicBool) -> SensorSnapshot {
            if request.enabled.iter().all(|&enabled| !enabled) {
                return SensorSnapshot::default();
            }
            let mut readings = Vec::new();
            let mut diagnostics = Vec::new();
            match monitor_readings(&mut self.lhm, stop) {
                Ok(values) => readings = values,
                Err(error) => diagnostics.push(error),
            }
            if readings.is_empty() {
                match monitor_readings(&mut self.ohm, stop) {
                    Ok(values) => readings = values,
                    Err(error) => diagnostics.push(error),
                }
            }
            if stop.load(Ordering::Relaxed) {
                return SensorSnapshot::default();
            }
            if request.enabled[2] && !readings.iter().any(|r| r.kind == Kind::Disk) {
                match storage_readings(&mut self.storage, stop) {
                    Ok(values) => readings.extend(values),
                    Err(error) => diagnostics.push(error),
                }
            }
            if stop.load(Ordering::Relaxed) {
                return SensorSnapshot::default();
            }
            if request.enabled[1]
                && !readings
                    .iter()
                    .any(|r| matches!(r.kind, Kind::GpuNvidia | Kind::GpuAmd | Kind::GpuIntel))
            {
                if self.nvml.is_none()
                    && self.nvml_retry.is_none_or(|retry| Instant::now() >= retry)
                {
                    match Nvml::new() {
                        Ok(nvml) => {
                            self.nvml = Some(nvml);
                            self.nvml_error = None;
                        }
                        Err(error) => {
                            self.nvml_error = Some(error);
                            self.nvml_retry = Some(Instant::now() + Duration::from_secs(30));
                        }
                    }
                }
                if let Some(nvml) = &self.nvml {
                    match nvml.readings() {
                        Ok(values) => readings.extend(values),
                        Err(error) => diagnostics.push(error),
                    }
                } else if let Some(error) = &self.nvml_error {
                    diagnostics.push(error.clone());
                }
            }
            readings.sort_by(|a, b| a.id.cmp(&b.id));
            snapshot(&readings, request, diagnostics)
        }
    }
    fn monitor_readings(
        provider: &mut Provider,
        stop: &AtomicBool,
    ) -> Result<Vec<Reading>, String> {
        let mut hardware = BTreeMap::new();
        for object in provider.query(
            "SELECT Identifier, Parent, Name, HardwareType FROM Hardware",
            stop,
        )? {
            let id = text(&object, "Identifier")?;
            let kind = hardware_kind(&text(&object, "HardwareType")?);
            let h = Hardware {
                id: id.clone(),
                parent: text(&object, "Parent")?,
                name: text(&object, "Name")?,
                kind,
            };
            hardware.insert(id, h);
        }
        let mut readings = Vec::new();
        for object in provider.query(
            "SELECT Identifier, Parent, Name, Value FROM Sensor WHERE SensorType='Temperature'",
            stop,
        )? {
            let Some(value) = number(&object, "Value")?.and_then(temperature) else {
                continue;
            };
            let parent = text(&object, "Parent")?;
            let Some(h) = classify(&hardware, &parent) else {
                continue;
            };
            let kind = h.kind.ok_or_else(|| "unclassified hardware".to_owned())?;
            readings.push(Reading {
                id: format!("{}:{}", provider.namespace, text(&object, "Identifier")?),
                device_id: format!("{}:{}", provider.namespace, h.id),
                device: if h.name.is_empty() {
                    h.id.clone()
                } else {
                    h.name.clone()
                },
                name: text(&object, "Name")?,
                kind,
                value,
            });
        }
        Ok(readings)
    }
    fn storage_readings(
        provider: &mut Provider,
        stop: &AtomicBool,
    ) -> Result<Vec<Reading>, String> {
        let counters = provider.query(
            "SELECT DeviceId, Temperature FROM MSFT_StorageReliabilityCounter",
            stop,
        )?;
        if counters.is_empty() {
            return Ok(Vec::new());
        }
        let mut names = BTreeMap::new();
        for object in
            provider.query("SELECT DeviceId, FriendlyName FROM MSFT_PhysicalDisk", stop)?
        {
            names.insert(text(&object, "DeviceId")?, text(&object, "FriendlyName")?);
        }
        let mut readings = Vec::new();
        for object in counters {
            let Some(value) = number(&object, "Temperature")?.and_then(temperature) else {
                continue;
            };
            let id = text(&object, "DeviceId")?;
            readings.push(Reading {
                id: format!("storage:{id}"),
                device_id: format!("storage:{id}"),
                device: names
                    .get(&id)
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("PhysicalDisk {id}")),
                name: "Temperature".into(),
                kind: Kind::Disk,
                value,
            });
        }
        Ok(readings)
    }
    struct Library(HMODULE);
    impl Drop for Library {
        fn drop(&mut self) {
            unsafe {
                FreeLibrary(self.0);
            }
        }
    }
    type Device = *mut c_void;
    type Initialize = unsafe extern "C" fn() -> i32;
    type Count = unsafe extern "C" fn(*mut u32) -> i32;
    type Handle = unsafe extern "C" fn(u32, *mut Device) -> i32;
    type Temperature = unsafe extern "C" fn(Device, u32, *mut u32) -> i32;
    type Name = unsafe extern "C" fn(Device, *mut i8, u32) -> i32;
    struct Nvml {
        shutdown: Initialize,
        count: Count,
        handle: Handle,
        temperature: Temperature,
        name: Name,
        _library: Library,
    }
    impl Nvml {
        fn new() -> Result<Self, String> {
            unsafe {
                let mut path = [0u16; 32768];
                let size = GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32);
                if size == 0 || size as usize >= path.len() {
                    return Err("Cannot locate system directory for NVML".into());
                }
                let mut file = path[..size as usize].to_vec();
                file.extend("\\nvml.dll".encode_utf16());
                file.push(0);
                let module = LoadLibraryExW(
                    file.as_ptr(),
                    ptr::null_mut(),
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
                );
                if module.is_null() {
                    return Err(format!(
                        "NVIDIA NVML is unavailable: {}",
                        std::io::Error::last_os_error()
                    ));
                }
                let library = Library(module);
                macro_rules! symbol {
                    ($name:literal,$ty:ty) => {{
                        let address = GetProcAddress(module, concat!($name, "\0").as_ptr())
                            .ok_or_else(|| format!("NVML export {} is unavailable", $name))?;
                        std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(address)
                    }};
                }
                let init = symbol!("nvmlInit_v2", Initialize);
                let shutdown = symbol!("nvmlShutdown", Initialize);
                let count = symbol!("nvmlDeviceGetCount_v2", Count);
                let handle = symbol!("nvmlDeviceGetHandleByIndex_v2", Handle);
                let temperature = symbol!("nvmlDeviceGetTemperature", Temperature);
                let name = symbol!("nvmlDeviceGetName", Name);
                let status = init();
                if status != 0 {
                    return Err(format!("NVML initialization failed: {status}"));
                }
                Ok(Self {
                    shutdown,
                    count,
                    handle,
                    temperature,
                    name,
                    _library: library,
                })
            }
        }
        fn readings(&self) -> Result<Vec<Reading>, String> {
            unsafe {
                let mut count = 0;
                let status = (self.count)(&mut count);
                if status != 0 {
                    return Err(format!("NVML device count failed: {status}"));
                }
                if count > 64 {
                    return Err("NVML device count exceeds 64".into());
                }
                let mut readings = Vec::new();
                for index in 0..count {
                    let mut device = ptr::null_mut();
                    let status = (self.handle)(index, &mut device);
                    if status != 0 {
                        return Err(format!("NVML device {index}: {status}"));
                    }
                    let mut degrees = 0;
                    let status = (self.temperature)(device, 0, &mut degrees);
                    if status == 3 {
                        continue;
                    }
                    if status != 0 {
                        return Err(format!("NVML device {index} temperature: {status}"));
                    }
                    let Some(value) = temperature(f64::from(degrees)) else {
                        continue;
                    };
                    let mut name = [0i8; 256];
                    let status = (self.name)(device, name.as_mut_ptr(), name.len() as u32);
                    if status != 0 {
                        return Err(format!("NVML device {index} name: {status}"));
                    }
                    name[255] = 0;
                    let device = CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned();
                    readings.push(Reading {
                        id: format!("nvml:{index}"),
                        device_id: format!("nvml:{index}"),
                        device,
                        name: "GPU Core".into(),
                        kind: Kind::GpuNvidia,
                        value,
                    });
                }
                Ok(readings)
            }
        }
    }
    impl Drop for Nvml {
        fn drop(&mut self) {
            unsafe {
                (self.shutdown)();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn average_uses_real_sensor_values() {
        assert_eq!(
            selected(&[("Core 1".into(), 40.), ("Core 2".into(), 60.)], "", true),
            Some(50.)
        );
    }
    #[test]
    fn named_selection_and_missing_name_follow_legacy_fallback() {
        let values = vec![("A".into(), 40.), ("B".into(), 60.)];
        assert_eq!(selected(&values, "B", true), Some(60.));
        assert_eq!(selected(&values, "gone", true), Some(40.));
        assert_eq!(selected(&values, "平均温度", true), Some(50.));
        assert_eq!(selected(&[], "", false), None);
    }
    #[test]
    fn missing_invalid_and_zero_values_are_not_temperatures() {
        for value in [0., -1., 201., f64::NAN, f64::INFINITY] {
            assert_eq!(temperature(value), None);
        }
        assert_eq!(temperature(42.5), Some(42.5));
    }
    #[test]
    fn motherboard_subhardware_is_classified_by_real_parent() {
        let mut map = BTreeMap::new();
        map.insert(
            "board".into(),
            Hardware {
                id: "board".into(),
                parent: String::new(),
                name: "Board".into(),
                kind: Some(Kind::Board),
            },
        );
        map.insert(
            "superio".into(),
            Hardware {
                id: "superio".into(),
                parent: "board".into(),
                name: "SuperIO".into(),
                kind: None,
            },
        );
        assert_eq!(classify(&map, "superio").unwrap().kind, Some(Kind::Board));
        assert!(classify(&map, "ACPI thermal zone").is_none());
    }
    fn reading(id: &str, name: &str, kind: Kind, value: f64) -> Reading {
        Reading {
            id: id.into(),
            device_id: format!("{kind:?}"),
            device: format!("{kind:?}"),
            name: name.into(),
            kind,
            value,
        }
    }
    #[test]
    fn duplicate_names_are_retained_and_gpu_prefers_core() {
        let readings = vec![
            reading("a", "Core", Kind::Cpu, 40.),
            reading("b", "Core", Kind::Cpu, 60.),
            reading("c", "GPU Hot Spot", Kind::GpuNvidia, 80.),
            reading("d", "GPU Core", Kind::GpuNvidia, 50.),
            reading("e", "GPU Core", Kind::GpuAmd, 55.),
        ];
        let result = snapshot(
            &readings,
            &Request {
                cpu: String::new(),
                disk: String::new(),
                enabled: [true; 4],
            },
            Vec::new(),
        );
        assert_eq!(result.cpu_cores.len(), 2);
        assert_eq!(result.cpu_temperature, Some(50.));
        assert_eq!(result.gpu_temperature, Some(50.));
        assert_eq!(result.hdd_temperature, None);
        assert_eq!(result.mainboard_temperature, None);
    }
    #[test]
    fn disabled_channels_never_publish_values() {
        let readings = vec![
            reading("a", "Core", Kind::Cpu, 40.),
            reading("b", "GPU Core", Kind::GpuNvidia, 50.),
            reading("c", "disk", Kind::Disk, 35.),
            reading("d", "board", Kind::Board, 30.),
        ];
        let result = snapshot(
            &readings,
            &Request {
                cpu: String::new(),
                disk: String::new(),
                enabled: [false; 4],
            },
            Vec::new(),
        );
        assert_eq!(result.cpu_temperature, None);
        assert_eq!(result.gpu_temperature, None);
        assert!(result.disks.is_empty());
        assert!(result.diagnostics.is_empty());
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    #[test]
    fn multiple_temperature_probes_on_one_disk_are_averaged() {
        let readings = vec![
            Reading {
                id: "disk/0/temp/0".into(),
                device_id: "disk/0".into(),
                device: "SSD".into(),
                name: "Composite".into(),
                kind: Kind::Disk,
                value: 40.,
            },
            Reading {
                id: "disk/0/temp/1".into(),
                device_id: "disk/0".into(),
                device: "SSD".into(),
                name: "Controller".into(),
                kind: Kind::Disk,
                value: 60.,
            },
        ];
        assert_eq!(
            named_values(&readings, Kind::Disk),
            vec![("SSD".into(), 50.)]
        );
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    fn cached(age: Duration) -> SensorSampler {
        let (tx, _requests) = mpsc::sync_channel(1);
        let (_results, rx) = mpsc::sync_channel(1);
        SensorSampler {
            requests: Some(tx),
            results: rx,
            last: Some(Stamped {
                snapshot: SensorSnapshot {
                    cpu_temperature: Some(50.),
                    cpu_cores: vec![("A".into(), 40.), ("B".into(), 60.)],
                    ..Default::default()
                },
                at: Instant::now() - age,
            }),
            previous: None,
            last_request: None,
            enabled: [true; 4],
            stop: Arc::new(AtomicBool::new(false)),
            worker_id: Arc::new(AtomicU32::new(0)),
            worker: None,
        }
    }
    #[test]
    fn changing_selected_core_uses_latest_owned_values_without_waiting() {
        let mut sampler = cached(Duration::ZERO);
        assert_eq!(sampler.sample("B", "").cpu_temperature, Some(60.));
    }
    #[test]
    fn expired_values_are_not_reported_as_current() {
        let mut sampler = cached(Duration::from_secs(11));
        let sample = sampler.sample("", "");
        assert_eq!(sample.cpu_temperature, None);
        assert!(sample.diagnostics.iter().any(|d| d.contains("expired")));
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;
    #[test]
    fn drop_waits_for_worker_owned_resources_to_be_released() {
        let (tx, _requests) = mpsc::sync_channel(1);
        let (_results, rx) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let released = Arc::new(AtomicBool::new(false));
        let released_worker = released.clone();
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                std::thread::yield_now();
            }
            std::thread::sleep(Duration::from_millis(30));
            released_worker.store(true, Ordering::Release);
        });
        let sampler = SensorSampler {
            requests: Some(tx),
            results: rx,
            last: None,
            previous: None,
            last_request: None,
            enabled: [true; 4],
            stop,
            worker_id: Arc::new(AtomicU32::new(0)),
            worker: Some(worker),
        };
        drop(sampler);
        assert!(
            released.load(Ordering::Acquire),
            "drop returned before worker cleanup"
        );
    }
}
