//! Crash evidence is written to the selected profile, never to a global directory.
//! In-process dumps are best effort: a corrupt stack or loader lock may prevent
//! DbgHelp from completing. This does not isolate or recover native plugin faults.
use std::sync::atomic::{AtomicBool, Ordering};
use std::{io, path::Path, sync::OnceLock};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{Diagnostics::Debug::*, Threading::*},
};
static CRASH_FILE: OnceLock<Vec<u16>> = OnceLock::new();
static DUMP_ACTIVE: AtomicBool = AtomicBool::new(false);
struct DumpGuard;
impl DumpGuard {
    fn acquire() -> io::Result<Self> {
        DUMP_ACTIVE
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map(|_| Self)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "a minidump is already being written",
                )
            })
    }
}
impl Drop for DumpGuard {
    fn drop(&mut self) {
        DUMP_ACTIVE.store(false, Ordering::Release);
    }
}
struct DumpFile(HANDLE);
impl Drop for DumpFile {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub fn install(directory: &Path) -> io::Result<()> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let path =
        std::path::absolute(directory)?.join(format!("crash-{stamp}-{}.dmp", std::process::id()));
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    CRASH_FILE.set(wide).map_err(|_| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "crash diagnostics already initialized",
        )
    })?;
    unsafe {
        SetUnhandledExceptionFilter(Some(exception_filter));
    }
    Ok(())
}
use std::os::windows::ffi::OsStrExt;
unsafe extern "system" fn exception_filter(exception: *const EXCEPTION_POINTERS) -> i32 {
    if let Some(path) = CRASH_FILE.get() {
        let _ = unsafe { dump(path, exception) };
    }
    // A corrupt plugin process is not resumed after collecting its evidence.
    EXCEPTION_CONTINUE_SEARCH
}
unsafe fn dump(path: &[u16], exception: *const EXCEPTION_POINTERS) -> io::Result<()> {
    // DbgHelp is single-threaded; never wait on a possibly faulting thread or
    // recurse into it if another exception occurs while it writes the dump.
    let _guard = DumpGuard::acquire()?;
    let file = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ,
            std::ptr::null(),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if file == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let file = DumpFile(file);
    let info = MINIDUMP_EXCEPTION_INFORMATION {
        ThreadId: unsafe { GetCurrentThreadId() },
        ExceptionPointers: exception.cast_mut(),
        ClientPointers: 0,
    };
    let success = unsafe {
        MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            file.0,
            MiniDumpNormal,
            if exception.is_null() {
                std::ptr::null()
            } else {
                &info
            },
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    let error = if success == 0 {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    if let Some(error) = error {
        Err(error)
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_minidump_has_signature_and_existing_evidence_is_never_replaced() {
        if std::env::var_os("TM_ISOLATED_DUMP_WORKER").as_deref() != Some(std::ffi::OsStr::new("1"))
        {
            use std::os::windows::process::CommandExt;
            let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/diagnostic-tests");
            std::fs::create_dir_all(&root).unwrap();
            let log = root.join(format!("dump-worker-{}.log", std::process::id()));
            let stdout = std::fs::File::create(&log).unwrap();
            let mut child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","diagnostics::tests::real_minidump_has_signature_and_existing_evidence_is_never_replaced","--test-threads=1"]).env("TM_ISOLATED_DUMP_WORKER","1").stdout(stdout.try_clone().unwrap()).stderr(stdout).creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW).spawn().unwrap();
            let start = std::time::Instant::now();
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(
                        status.success(),
                        "{}",
                        std::fs::read_to_string(&log).unwrap_or_default()
                    );
                    break;
                }
                if start.elapsed() > std::time::Duration::from_secs(20) {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("isolated minidump worker timed out; parent tests remain responsive");
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            std::fs::remove_file(log).unwrap();
            return;
        }
        // MiniDumpWriteDump suspends peer threads. Keep this real dump out of the
        // process loading PDH/COM providers for unrelated parallel tests.
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/diagnostic-tests");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "{}-{}.dmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            dump(&wide, std::ptr::null()).unwrap();
        }
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"MDMP");
        assert!(bytes.len() > 32);
        assert!(unsafe { dump(&wide, std::ptr::null()) }.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_file(&path).unwrap();
        let guard = DumpGuard::acquire().unwrap();
        assert_eq!(
            unsafe { dump(&wide, std::ptr::null()) }.unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(guard);
        // Releasing the guard permits a subsequent dump; the HANDLE is also closed.
        unsafe {
            dump(&wide, std::ptr::null()).unwrap();
        }
        std::fs::remove_file(path).unwrap();
    }
}
