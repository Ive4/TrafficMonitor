fn main() {
    for name in ["RC", "ProgramFiles(x86)", "WindowsSdkDir", "HOST"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=../../TrafficMonitor/res/TrafficMonitor.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let package = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let manifest = package.join("app.manifest");
        println!("cargo:rustc-link-arg-bin=TrafficMonitor=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=TrafficMonitor=/MANIFESTINPUT:{}",
            manifest.display()
        );
        let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
            .join("TrafficMonitor.res");
        let compiler = std::env::var_os("RC")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                let root = std::env::var_os("WindowsSdkDir")
                    .map(std::path::PathBuf::from)
                    .or_else(|| {
                        std::env::var_os("ProgramFiles(x86)")
                            .map(|p| std::path::PathBuf::from(p).join("Windows Kits/10"))
                    })?
                    .join("bin");
                let mut versions: Vec<_> = std::fs::read_dir(root)
                    .ok()?
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .collect();
                versions.sort();
                versions.reverse();
                // rc.exe runs on the build host, regardless of the target DLL/EXE architecture.
                let host = std::env::var("HOST").unwrap_or_default();
                let arches: &[&str] = if host.starts_with("aarch64-") {
                    &["arm64", "x64", "x86"]
                } else if host.starts_with("i686-") {
                    &["x86"]
                } else {
                    &["x64", "x86"]
                };
                versions
                    .into_iter()
                    .flat_map(|v| arches.iter().map(move |arch| v.join(arch).join("rc.exe")))
                    .find(|p| p.is_file())
            })
            .unwrap_or_else(|| "rc.exe".into());
        let mut command = std::process::Command::new(compiler);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW for the noninteractive SDK helper.
        }
        let status = command
            .current_dir(&package)
            .arg("/nologo")
            .arg("/fo")
            .arg(&output)
            .arg(package.join("app.rc"))
            .status()
            .expect("Windows SDK resource compiler is required");
        assert!(status.success(), "compiling icon/version resource failed");
        println!(
            "cargo:rustc-link-arg-bin=TrafficMonitor={}",
            output.display()
        );
    }
}
