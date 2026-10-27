use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use traffic_monitor_config::{AppPaths, IniDocument};

/// Each fixture owns one newly created directory; cleanup checks its resolved parent.
struct Directory {
    path: PathBuf,
    parent: PathBuf,
    name: std::ffi::OsString,
}
impl Directory {
    fn in_parent(parent: &Path, prefix: &str) -> Self {
        let name = format!(
            "{prefix}-TrafficMonitor-path-policy-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = parent.join(&name);
        fs::create_dir(&path).unwrap();
        Self {
            path,
            parent: parent.canonicalize().unwrap(),
            name: name.into(),
        }
    }
    fn temporary() -> Self {
        Self::in_parent(&std::env::temp_dir(), "owned")
    }
    fn module(&self) -> PathBuf {
        let p = self.path.join("extracted");
        fs::create_dir(&p).unwrap();
        p
    }
    fn profile(&self) -> PathBuf {
        self.path.join("roaming")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if let Ok(path) = self.path.canonicalize() {
            assert_eq!(path.parent(), Some(self.parent.as_path()));
            assert_eq!(path.file_name(), Some(self.name.as_os_str()));
            fs::remove_dir_all(path).unwrap();
        }
    }
}

#[test]
fn writable_temporary_installation_uses_roaming_profile() {
    let directory = Directory::temporary();
    let paths = AppPaths::resolve(directory.module(), directory.profile(), true).unwrap();
    assert!(
        !paths.portable,
        "temporary extraction must not become a disposable profile"
    );
    assert_eq!(
        paths.config_file,
        directory.profile().join("TrafficMonitor/config.ini")
    );
    assert_eq!(
        paths.history_file,
        directory
            .profile()
            .join("TrafficMonitor/history_traffic.dat")
    );
}
#[test]
fn temporary_guard_also_overrides_automatic_portable_switch() {
    let directory = Directory::temporary();
    let module = directory.module();
    let mut ini = IniDocument::default();
    ini.set_bool("config", "portable_mode", true);
    ini.save_atomic(module.join("global_cfg.ini")).unwrap();
    let paths = AppPaths::resolve(module, directory.profile(), true).unwrap();
    assert!(!paths.portable);
}
#[test]
fn temporary_comparison_resolves_parent_components() {
    let directory = Directory::temporary();
    let module = directory.module();
    let with_parent = module.join("..").join("extracted");
    let paths = AppPaths::resolve(with_parent, directory.profile(), true).unwrap();
    assert!(!paths.portable);
}
#[test]
fn a_sibling_with_the_same_text_prefix_remains_portable() {
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    let parent = temporary.parent().unwrap();
    let prefix = temporary.file_name().unwrap().to_string_lossy();
    let directory = Directory::in_parent(parent, &prefix);
    let paths = AppPaths::resolve(directory.module(), directory.profile(), true).unwrap();
    assert!(
        paths.portable,
        "Temp suffix must be compared as components, not a string prefix"
    );
}
#[cfg(windows)]
#[test]
fn temporary_comparison_is_case_insensitive_on_windows() {
    let directory = Directory::temporary();
    let module = directory.module().to_string_lossy().to_uppercase();
    let paths = AppPaths::resolve(module.into(), directory.profile(), true).unwrap();
    assert!(!paths.portable);
}
