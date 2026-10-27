use crate::IniDocument;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub module_dir: PathBuf,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub history_file: PathBuf,
    pub error_log: PathBuf,
    pub skins_dir: PathBuf,
    pub language_dir: PathBuf,
    pub plugins_dir: PathBuf,
    pub portable: bool,
}
impl AppPaths {
    pub fn resolve(
        module_dir: PathBuf,
        appdata_root: PathBuf,
        module_writable: bool,
    ) -> io::Result<Self> {
        let appdata = appdata_root.join("TrafficMonitor");
        let global = module_dir.join("global_cfg.ini");
        // A writable archive extraction is still disposable. Match the original
        // application's roaming-profile policy, using resolved path components
        // so TempSibling directories are not mistaken for the system Temp root.
        let module_writable =
            module_writable && !directory_is_within(&module_dir, &std::env::temp_dir());
        let portable = if global.exists() {
            IniDocument::load(&global)?.get_bool("config", "portable_mode", false)
        } else {
            !appdata.join("config.ini").exists()
        } && module_writable;
        let config_dir = if portable {
            module_dir.clone()
        } else {
            appdata
        };
        Ok(Self {
            config_file: config_dir.join("config.ini"),
            history_file: config_dir.join("history_traffic.dat"),
            error_log: config_dir.join("error.log"),
            skins_dir: module_dir.join("skins"),
            language_dir: module_dir.join("language"),
            plugins_dir: module_dir.join("plugins"),
            module_dir,
            config_dir,
            portable,
        })
    }
    pub fn discover() -> io::Result<Self> {
        let module = std::env::current_exe()?
            .parent()
            .ok_or_else(|| io::Error::other("executable has no parent"))?
            .to_owned();
        let app = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "APPDATA is unset"))?;
        let writable = directory_writable(&module);
        Self::resolve(module, app, writable)
    }
    pub fn ensure_config_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.config_dir)
    }
    pub fn plugin_config_dir(&self) -> PathBuf {
        self.config_dir.join("plugins")
    }
    pub fn resource(&self, name: impl AsRef<Path>) -> PathBuf {
        self.module_dir.join(name)
    }
    /// Explicit migration copies only missing files, never moves or overwrites a source.
    pub fn migrate_from(&self, old_dir: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
        self.ensure_config_dir()?;
        let mut copied = Vec::new();
        for name in [
            "config.ini",
            "history_traffic.dat",
            "history_traffic.dat.bak",
        ] {
            let from = old_dir.as_ref().join(name);
            let to = self.config_dir.join(name);
            if from.is_file() && !to.exists() {
                let mut input = fs::File::open(&from)?;
                let mut output = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&to)?;
                std::io::copy(&mut input, &mut output)?;
                output.sync_all()?;
                copied.push(to);
            }
        }
        Ok(copied)
    }
}
fn directory_is_within(directory: &Path, root: &Path) -> bool {
    let (Ok(directory), Ok(root)) = (directory.canonicalize(), root.canonicalize()) else {
        // discover() supplies an existing executable directory. A missing root
        // cannot contain it; pure resolve() callers may also model future paths.
        return false;
    };
    let mut directory = directory.components();
    root.components().all(|component| {
        directory
            .next()
            .is_some_and(|candidate| same_component(candidate.as_os_str(), component.as_os_str()))
    })
}
#[cfg(windows)]
fn same_component(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
    let left: Vec<u16> = left.encode_wide().collect();
    let right: Vec<u16> = right.encode_wide().collect();
    // Paths are already bounded by the filesystem. Compare UTF-16 directly to
    // retain names that contain unpaired surrogates and Windows case semantics.
    unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        ) == CSTR_EQUAL
    }
}
#[cfg(not(windows))]
fn same_component(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> bool {
    left == right
}
fn directory_writable(path: &Path) -> bool {
    let p = path.join(format!(
        ".traffic-monitor-write-probe-{}",
        std::process::id()
    ));
    match fs::OpenOptions::new().write(true).create_new(true).open(&p) {
        Ok(file) => {
            drop(file);
            let _ = fs::remove_file(p);
            true
        }
        Err(_) => false,
    }
}
