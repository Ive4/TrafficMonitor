use super::*;

struct HostStrings {
    version: Vec<u16>,
    config: Vec<u16>,
    main: [Vec<u16>; 13],
    taskbar: [Vec<u16>; 13],
    resources: BTreeMap<(String, String), Vec<u16>>,
}
impl HostStrings {
    fn new(info: &HostInfo) -> Self {
        Self {
            version: wide(&info.version),
            config: wide(&config_path(info)),
            main: std::array::from_fn(|i| wide(&info.main_value_strings[i])),
            taskbar: std::array::from_fn(|i| wide(&info.taskbar_value_strings[i])),
            resources: info
                .strings
                .iter()
                .map(|(k, v)| (k.clone(), wide(v)))
                .collect(),
        }
    }
}
pub(super) fn config_path(info: &HostInfo) -> String {
    let mut path = info.config_dir.to_string_lossy().into_owned();
    if !path.is_empty() && !path.ends_with(['\\', '/']) {
        path.push('\\');
    }
    path
}
#[repr(C)]
pub(super) struct Host {
    vtable: *const *const c_void,
    owner: u32,
    live: Cell<bool>,
    info: HostInfo,
    strings: HostStrings,
    notifications: RefCell<Vec<String>>,
}
impl Host {
    pub(super) fn new(info: HostInfo) -> Self {
        Self {
            vtable: TABLE.0.as_ptr(),
            owner: unsafe { GetCurrentThreadId() },
            live: Cell::new(true),
            strings: HostStrings::new(&info),
            info,
            notifications: RefCell::new(Vec::new()),
        }
    }
    pub(super) fn info(&self) -> &HostInfo {
        &self.info
    }
    pub(super) fn update(&mut self, info: HostInfo) {
        self.strings = HostStrings::new(&info);
        self.info = info;
    }
    pub(super) fn close(&self) {
        self.live.set(false);
    }
    pub(super) fn take_notifications(&self) -> Vec<String> {
        std::mem::take(&mut *self.notifications.borrow_mut())
    }
}
unsafe fn host<'a>(p: *mut c_void) -> Option<&'a Host> {
    if p.is_null() {
        return None;
    }
    let h = &*(p as *const Host);
    // Check immutable thread ID before accessing any thread-affine state.
    if h.owner != GetCurrentThreadId() || !h.live.get() {
        None
    } else {
        Some(h)
    }
}
static EMPTY: [u16; 1] = [0];
callback! {
    fn api(_p:*mut c_void)->i32 {1}
    fn version(p:*mut c_void)->*const u16 {host(p).map_or(EMPTY.as_ptr(),|h|h.strings.version.as_ptr())}
    fn value(p:*mut c_void,item:i32)->f64 {host(p).and_then(|h|h.info.monitor_values.get(item as usize).copied()).unwrap_or(0.)}
    fn value_string(p:*mut c_void,item:i32,main:i32)->*const u16 {
        host(p).and_then(|h|if main!=0 {h.strings.main.get(item as usize)}else{h.strings.taskbar.get(item as usize)}).map_or(EMPTY.as_ptr(),|s|s.as_ptr())
    }
    fn notify(p:*mut c_void,msg:*const u16)->() {
        if let Some(h)=host(p) {if let Ok(s)=string(msg) {h.notifications.borrow_mut().push(s);}}
    }
    fn language(p:*mut c_void)->u16 {host(p).map_or(0,|h|h.info.language_id)}
    fn config(p:*mut c_void)->*const u16 {host(p).map_or(EMPTY.as_ptr(),|h|h.strings.config.as_ptr())}
    fn dpi(p:*mut c_void,kind:i32)->i32 {host(p).map_or(96,|h|if kind==1{h.info.taskbar_dpi}else{h.info.main_dpi})}
    fn theme(p:*mut c_void)->u32 {host(p).map_or(0,|h|h.info.theme_color)}
    fn resource(p:*mut c_void,key:*const u16,section:*const u16)->*const u16 {
        host(p).and_then(|h|{let k=string(key).ok()?;let s=string(section).ok()?;h.strings.resources.get(&(k,s))}).map_or(EMPTY.as_ptr(),|s|s.as_ptr())
    }
    fn main_window(p:*mut c_void)->*mut c_void {host(p).map_or(ptr::null_mut(),|h|h.info.main_window as *mut c_void)}
    fn taskbar_window(p:*mut c_void)->*mut c_void {host(p).map_or(ptr::null_mut(),|h|h.info.taskbar_window as *mut c_void)}
}
struct VTable([*const c_void; 12]);
// Immutable function addresses, no mutable data behind the table.
unsafe impl Sync for VTable {}
static TABLE: VTable = VTable([
    api as _,
    version as _,
    value as _,
    value_string as _,
    notify as _,
    language as _,
    config as _,
    dpi as _,
    theme as _,
    resource as _,
    main_window as _,
    taskbar_window as _,
]);
