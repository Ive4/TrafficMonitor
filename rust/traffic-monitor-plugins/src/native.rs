#![allow(unsafe_op_in_unsafe_fn)]
use super::*;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::path::Path;
use std::ptr;
use std::rc::Rc;
use windows_sys::Win32::Foundation::{FreeLibrary, GetLastError, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    LoadLibraryExW,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

// MSVC single inheritance has one vptr and no destructor slot in these interfaces.
// x86 members use ECX thiscall; x64/ARM64 Windows use their unified system ABI.
macro_rules! call {
    ($obj:expr,$slot:expr,($($ty:ty),*),($($arg:expr),*),$ret:ty) => {{
        #[cfg(target_arch="x86")]
        type Method = unsafe extern "thiscall" fn(*mut c_void, $($ty),*) -> $ret;
        #[cfg(not(target_arch="x86"))]
        type Method = unsafe extern "system" fn(*mut c_void, $($ty),*) -> $ret;
        let obj = $obj;
        let addr = slot(obj,$slot)?;
        #[allow(unused_unsafe)] let method: Method = unsafe { std::mem::transmute(addr) };
        #[allow(unused_unsafe)] let result=unsafe { method(obj,$($arg),*) }; result
    }};
}
macro_rules! callback {
    ($(fn $name:ident($($arg:ident:$ty:ty),*) -> $ret:ty $body:block)*) => {$(
        #[cfg(target_arch="x86")]
        unsafe extern "thiscall" fn $name($($arg:$ty),*) -> $ret $body
        #[cfg(not(target_arch="x86"))]
        unsafe extern "system" fn $name($($arg:$ty),*) -> $ret $body
    )*};
}
mod drawer;
mod host;
use host::Host;

fn slot(obj: *mut c_void, index: usize) -> Result<*const c_void, String> {
    if obj.is_null() {
        return Err("plugin returned a null object".into());
    }
    unsafe {
        let table = *(obj as *const *const *const c_void);
        if table.is_null() {
            return Err("plugin returned a null vtable".into());
        }
        let entry = *table.add(index);
        if entry.is_null() {
            return Err(format!("plugin vtable slot {index} is null"));
        }
        Ok(entry)
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
unsafe fn string(p: *const u16) -> Result<String, String> {
    if p.is_null() {
        return Ok(String::new());
    }
    for n in 0..65536 {
        if *p.add(n) == 0 {
            return Ok(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
        }
    }
    Err("plugin string exceeds 65535 UTF-16 code units".into())
}
fn text(obj: *mut c_void, index: usize) -> Result<String, String> {
    unsafe { string(call!(obj, index, (), (), *const u16)) }
}
struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                FreeLibrary(self.0);
            }
        }
    }
}
struct Loaded {
    _library: Library,
    object: *mut c_void,
    items: Vec<*mut c_void>,
}
struct Entry {
    info: PluginInfo,
    loaded: Option<Loaded>,
}

/// UI-thread-only native plugin service. !Send/!Sync prevents cross-thread moves.
/// Loading a DLL executes trusted native code. SEH/C++ exceptions are not isolated.
pub struct PluginManager {
    entries: Vec<Entry>,
    host: Box<Host>,
    owner: u32,
    closed: bool,
    _thread_affinity: PhantomData<Rc<()>>,
    graphics: Result<Graphics, String>,
}
impl PluginManager {
    pub fn new(info: HostInfo) -> Self {
        Self {
            entries: Vec::new(),
            host: Box::new(Host::new(info)),
            owner: unsafe { GetCurrentThreadId() },
            closed: false,
            _thread_affinity: PhantomData,
            graphics: Graphics::new(),
        }
    }
    fn check(&self) -> Result<(), String> {
        if self.closed {
            return Err("plugin manager is shut down".into());
        }
        if unsafe { GetCurrentThreadId() } != self.owner {
            return Err("plugin calls require the creating UI thread".into());
        }
        Ok(())
    }
    pub fn discover(&mut self, dir: &Path, disabled: &[String]) -> Result<(), String> {
        self.check()?;
        if !dir.exists() {
            return Ok(());
        }
        let root = dir.canonicalize().map_err(|e| e.to_string())?;
        let mut paths = Vec::new();
        for f in std::fs::read_dir(&root).map_err(|e| e.to_string())? {
            let path = f.map_err(|e| e.to_string())?.path();
            if path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("dll"))
                && path.is_file()
            {
                let canonical = path.canonicalize().map_err(|e| e.to_string())?;
                if canonical.parent() != Some(root.as_path()) {
                    return Err(format!("plugin path escapes directory: {}", path.display()));
                }
                paths.push(canonical);
            }
        }
        paths.sort();
        for p in paths {
            let id = file_id(&p)?;
            if self
                .entries
                .iter()
                .any(|e| e.info.id.eq_ignore_ascii_case(&id))
            {
                continue;
            }
            if disabled.iter().any(|d| d.eq_ignore_ascii_case(&id)) {
                self.entries.push(Entry {
                    info: blank_info(p, id, false),
                    loaded: None,
                });
            } else {
                let _ = self.load(&p); /* error retained in PluginInfo for the UI */
            }
        }
        Ok(())
    }
    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        self.check()?;
        let path = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let id = file_id(&path)?;
        if self
            .entries
            .iter()
            .any(|e| e.info.id.eq_ignore_ascii_case(&id))
        {
            return Err(format!("duplicate plugin {id}"));
        }
        let mut entry = Entry {
            info: blank_info(path, id, true),
            loaded: None,
        };
        let result = self.load_entry(&mut entry);
        if let Err(e) = &result {
            entry.info.error = Some(e.clone());
        }
        self.entries.push(entry);
        result
    }
    fn load_entry(&mut self, entry: &mut Entry) -> Result<(), String> {
        self.graphics.as_ref().map_err(Clone::clone)?;
        if !entry.info.path.is_absolute() {
            return Err("plugin DLL path must be absolute".into());
        }
        let filename = wide(&entry.info.path.to_string_lossy());
        let module = unsafe {
            LoadLibraryExW(
                filename.as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        };
        if module.is_null() {
            return Err(format!("LoadLibraryExW failed: {}", unsafe {
                GetLastError()
            }));
        }
        let library = Library(module);
        let address = unsafe { GetProcAddress(module, c"TMPluginGetInstance".as_ptr().cast()) }
            .ok_or_else(|| {
                format!("TMPluginGetInstance export missing: {}", unsafe {
                    GetLastError()
                })
            })?;
        let get: unsafe extern "C" fn() -> *mut c_void = unsafe { std::mem::transmute(address) };
        let object = unsafe { get() };
        let version = call!(object, 0, (), (), i32);
        if !supports_api(version) {
            return Err(format!(
                "unsupported plugin API version {version}; supported 1..=8"
            ));
        }
        entry.info.api_version = version;
        if version >= 7 {
            call!(
                object,
                14,
                (*mut c_void),
                (self.host.as_mut() as *mut Host as *mut c_void),
                ()
            );
        }
        send_extended(object, version, self.host.info())?;
        for i in 0..6 {
            entry.info.properties[i] =
                unsafe { string(call!(object, 4, (i32), (i as i32), *const u16)) }?;
        }
        let mut items = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for i in 0..=1024 {
            let item = call!(object, 1, (i32), (i), *mut c_void);
            if item.is_null() {
                break;
            }
            if i == 1024 {
                return Err("plugin exceeds 1024 items or lacks terminating null".into());
            }
            let id = text(item, 1)?;
            if id.is_empty()
                || !ids.insert(id.clone())
                || self
                    .entries
                    .iter()
                    .filter_map(|e| e.loaded.as_ref())
                    .flat_map(|l| &l.items)
                    .any(|&p| text(p, 1).is_ok_and(|s| s == id))
            {
                return Err(format!("empty or duplicate plugin item ID {id:?}"));
            }
            items.push(item);
        }
        entry.info.commands = commands(object, version)?;
        entry.info.error = None;
        entry.loaded = Some(Loaded {
            _library: library,
            object,
            items,
        });
        Ok(())
    }
    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.entries.iter().map(|e| e.info.clone()).collect()
    }
    pub fn update_host(&mut self, info: HostInfo) -> Result<(), String> {
        self.check()?;
        let previous = self.host.info().clone();
        self.host.update(info);
        for e in &self.entries {
            if let Some(l) = &e.loaded {
                send_changed_extended(l.object, e.info.api_version, &previous, self.host.info())?;
            }
        }
        Ok(())
    }
    pub fn take_notifications(&mut self) -> Vec<String> {
        self.host.take_notifications()
    }
    pub fn refresh(&mut self, monitor: MonitorInfo) -> Result<(), String> {
        self.check()?;
        for e in &mut self.entries {
            if let Some(l) = &e.loaded {
                call!(l.object, 5, (*const MonitorInfo), (&monitor), ());
                call!(l.object, 2, (), (), ());
                e.info.commands = commands(l.object, e.info.api_version)?;
            }
        }
        Ok(())
    }
    pub fn items(&mut self) -> Result<Vec<PluginItem>, String> {
        self.check()?;
        let dpi = self.host.info().main_dpi;
        let mut out = Vec::new();
        for e in &self.entries {
            if let Some(l) = &e.loaded {
                for &item in &l.items {
                    let custom_draw = call!(item, 5, (), (), bool);
                    let raw_width = call!(item, 6, (), (), i32);
                    let graph = if e.info.api_version >= 6 && call!(item, 12, (), (), i32) != 0 {
                        Some(call!(item, 13, (), (), f32))
                    } else {
                        None
                    };
                    out.push(PluginItem {
                        id: text(item, 1)?,
                        plugin_id: e.info.id.clone(),
                        name: text(item, 0)?,
                        label: text(item, 2)?,
                        value: text(item, 3)?,
                        sample: text(item, 4)?,
                        custom_draw,
                        width: raw_width.saturating_mul(dpi) / 96,
                        graph: graph.filter(|v| v.is_finite()).map(|v| v.clamp(0., 1.)),
                        double_line: e.info.api_version >= 8 && call!(item, 15, (), (), i32) != 0,
                    });
                }
            }
        }
        Ok(out)
    }
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        self.check()?;
        let index = self
            .entries
            .iter()
            .position(|e| e.info.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| format!("unknown plugin {id}"))?;
        if !enabled {
            let e = &mut self.entries[index];
            e.loaded = None;
            e.info.enabled = false;
            e.info.commands.clear();
            return Ok(());
        }
        if self.entries[index].loaded.is_some() {
            return Ok(());
        }
        let mut entry = self.entries.remove(index);
        entry.info.enabled = true;
        let result = self.load_entry(&mut entry);
        entry.info.error = result.as_ref().err().cloned();
        self.entries.insert(index, entry);
        result
    }
    fn plugin(&self, id: &str) -> Result<(&Entry, &Loaded), String> {
        self.check()?;
        let e = self
            .entries
            .iter()
            .find(|e| e.info.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| format!("unknown plugin {id}"))?;
        Ok((
            e,
            e.loaded
                .as_ref()
                .ok_or_else(|| format!("plugin {id} is not loaded"))?,
        ))
    }
    fn item(&self, id: &str) -> Result<(&Entry, &Loaded, *mut c_void), String> {
        self.check()?;
        for e in &self.entries {
            if let Some(l) = &e.loaded {
                for &p in &l.items {
                    if text(p, 1)? == id {
                        return Ok((e, l, p));
                    }
                }
            }
        }
        Err(format!("unknown plugin item {id}"))
    }
    pub fn configure(&mut self, id: &str, parent: usize) -> Result<OptionResult, String> {
        let (_, l) = self.plugin(id)?;
        match call!(l.object, 3, (*mut c_void), (parent as *mut c_void), i32) {
            0 => Ok(OptionResult::Changed),
            1 => Ok(OptionResult::Unchanged),
            2 => Ok(OptionResult::NotProvided),
            n => Err(format!("invalid options result {n}")),
        }
    }
    pub fn command(&mut self, id: &str, index: usize, parent: usize) -> Result<(), String> {
        let (e, l) = self.plugin(id)?;
        if e.info.api_version < 5 || index >= e.info.commands.len() {
            return Err(format!("unsupported plugin command {index}"));
        }
        call!(
            l.object,
            12,
            (i32, *mut c_void, *mut c_void),
            (index as i32, parent as *mut c_void, ptr::null_mut()),
            ()
        );
        Ok(())
    }
    pub fn tooltip(&mut self, id: &str) -> Result<String, String> {
        let (e, l) = self.plugin(id)?;
        if e.info.api_version < 2 {
            return Ok(String::new());
        }
        text(l.object, 6)
    }
    /// Borrowed HICON owned by the DLL. Valid only while this plugin is loaded.
    pub fn plugin_icon(&mut self, id: &str) -> Result<usize, String> {
        let (e, l) = self.plugin(id)?;
        if e.info.api_version < 2 {
            return Ok(0);
        }
        Ok(call!(l.object, 8, (), (), *mut c_void) as usize)
    }
    pub fn mouse(&mut self, id: &str, event: MouseEvent) -> Result<bool, String> {
        let (e, _, p) = self.item(id)?;
        if e.info.api_version < 3 {
            return Ok(false);
        }
        Ok(call!(
            p,
            9,
            (i32, i32, i32, *mut c_void, i32),
            (
                event.kind as i32,
                event.x,
                event.y,
                event.window as *mut c_void,
                i32::from(event.taskbar)
            ),
            i32
        ) != 0)
    }
    pub fn keyboard(&mut self, id: &str, event: KeyboardEvent) -> Result<bool, String> {
        let (e, _, p) = self.item(id)?;
        if e.info.api_version < 4 {
            return Ok(false);
        }
        Ok(call!(
            p,
            10,
            (i32, bool, bool, bool, *mut c_void, i32),
            (
                event.key,
                event.ctrl,
                event.shift,
                event.alt,
                event.window as *mut c_void,
                i32::from(event.taskbar)
            ),
            i32
        ) != 0)
    }
    pub fn item_width(&mut self, id: &str, hdc: usize, taskbar: bool) -> Result<i32, String> {
        let (e, _, p) = self.item(id)?;
        if hdc == 0 {
            return Err("width measurement requires a valid HDC".into());
        }
        if e.info.api_version >= 3 {
            let w = call!(p, 8, (*mut c_void), (hdc as *mut c_void), i32);
            if w > 0 {
                return Ok(w);
            }
        }
        let info = self.host.info();
        let dpi = if taskbar {
            info.taskbar_dpi
        } else {
            info.main_dpi
        };
        Ok(call!(p, 6, (), (), i32).saturating_mul(dpi) / 96)
    }
    /// Send the current item's COLORREF label/value colors immediately before draw.
    /// Only its owning API >= 2 plugin receives EI 0/1; no host formats change.
    pub fn set_draw_colors(&mut self, item_id: &str, label: u32, value: u32) -> Result<(), String> {
        let (entry, loaded, _) = self.item(item_id)?;
        if entry.info.api_version >= 2 {
            for (kind, color) in [(0, label), (1, value)] {
                let text = wide(&color.to_string());
                call!(
                    loaded.object,
                    7,
                    (i32, *const u16),
                    (kind, text.as_ptr()),
                    ()
                );
            }
        }
        Ok(())
    }
    pub fn draw(
        &mut self,
        id: &str,
        hdc: usize,
        rect: [i32; 4],
        dark: bool,
        taskbar: bool,
    ) -> Result<bool, String> {
        let (e, l, p) = self.item(id)?;
        if !call!(p, 5, (), (), bool) {
            return Ok(false);
        }
        if hdc == 0 || rect[2] <= 0 || rect[3] <= 0 {
            return Err("drawing requires a valid HDC and positive dimensions".into());
        }
        if e.info.api_version >= 2 {
            let value = wide(if taskbar { "1" } else { "0" });
            call!(l.object, 7, (i32, *const u16), (2, value.as_ptr()), ());
        }
        let saved = unsafe { windows_sys::Win32::Graphics::Gdi::SaveDC(hdc as _) };
        if saved == 0 {
            return Err("SaveDC failed before plugin draw".into());
        }
        let result = (|| {
            let [x, y, w, h] = rect;
            call!(
                p,
                7,
                (*mut c_void, i32, i32, i32, i32, bool),
                (hdc as *mut c_void, x, y, w, h, dark),
                ()
            );
            if e.info.api_version >= 8 {
                let mut drawer = drawer::Drawer::new(hdc);
                let _handled = call!(
                    p,
                    14,
                    (*mut c_void, i32, i32, i32, i32, bool),
                    (&mut drawer as *mut _ as *mut c_void, x, y, w, h, dark),
                    bool
                );
                // Original host always runs legacy DrawItem too: API8 may split GDI+ content across both callbacks.
                drawer.finish()?;
            }
            Ok(true)
        })();
        if unsafe { windows_sys::Win32::Graphics::Gdi::RestoreDC(hdc as _, saved) } == 0 {
            return Err("RestoreDC failed after plugin draw".into());
        }
        result
    }
    pub fn shutdown(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        self.check()?;
        self.host.close();
        // End host callbacks before unloading DLLs, retaining host allocation during DllMain.
        for e in &mut self.entries {
            e.loaded = None;
        }
        self.closed = true;
        Ok(())
    }
}
impl Drop for PluginManager {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
fn file_id(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "plugin filename must be valid Unicode".into())
}
fn blank_info(path: PathBuf, id: String, enabled: bool) -> PluginInfo {
    PluginInfo {
        id,
        path,
        enabled,
        api_version: 0,
        properties: std::array::from_fn(|_| String::new()),
        commands: Vec::new(),
        error: None,
    }
}
fn commands(p: *mut c_void, version: i32) -> Result<Vec<PluginCommand>, String> {
    if version < 5 {
        return Ok(Vec::new());
    }
    let count = call!(p, 9, (), (), i32);
    if !(0..=1024).contains(&count) {
        return Err(format!("invalid plugin command count {count}"));
    }
    (0..count)
        .map(|i| {
            Ok(PluginCommand {
                index: i as usize,
                name: unsafe { string(call!(p, 10, (i32), (i), *const u16)) }?,
                icon: call!(p, 11, (i32), (i), *mut c_void) as usize,
                checked: call!(p, 13, (i32), (i), i32) != 0,
            })
        })
        .collect()
}
fn send_extended(object: *mut c_void, version: i32, info: &HostInfo) -> Result<(), String> {
    if version < 2 {
        return Ok(());
    }
    if !info.config_dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&info.config_dir)
            .map_err(|e| format!("plugin config directory: {e}"))?;
    }
    let path = host::config_path(info);
    let value = wide(&path);
    call!(object, 7, (i32, *const u16), (17, value.as_ptr()), ());
    for (&i, v) in &info.extended {
        if (0..17).contains(&i) {
            let value = wide(v);
            call!(object, 7, (i32, *const u16), (i, value.as_ptr()), ());
        }
    }
    Ok(())
}

struct Graphics(usize);
impl Graphics {
    fn new() -> Result<Self, String> {
        use windows_sys::Win32::Graphics::GdiPlus::{GdiplusStartup, GdiplusStartupInput};
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token = 0;
        let status = unsafe { GdiplusStartup(&mut token, &input, ptr::null_mut()) };
        if status == 0 {
            Ok(Self(token))
        } else {
            Err(format!("GdiplusStartup failed: {status}"))
        }
    }
}
impl Drop for Graphics {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Graphics::GdiPlus::GdiplusShutdown(self.0);
        }
    }
}

fn send_changed_extended(
    object: *mut c_void,
    version: i32,
    previous: &HostInfo,
    info: &HostInfo,
) -> Result<(), String> {
    if version < 2 {
        return Ok(());
    }
    if previous.config_dir != info.config_dir {
        if !info.config_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&info.config_dir).map_err(|e| e.to_string())?;
        }
        let path = wide(&host::config_path(info));
        call!(object, 7, (i32, *const u16), (17, path.as_ptr()), ());
    }
    for (&i, v) in &info.extended {
        if (0..17).contains(&i) && previous.extended.get(&i) != Some(v) {
            let value = wide(v);
            call!(object, 7, (i32, *const u16), (i, value.as_ptr()), ());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests;
