use std::{
    marker::PhantomData,
    ptr::null_mut,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use windows_sys::Win32::{
    Foundation::{
        GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SetLastError, WPARAM,
    },
    Graphics::Gdi::ClientToScreen,
    System::{
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
        Threading::{GetCurrentProcessId, GetCurrentThreadId},
    },
    UI::{
        HiDpi::{
            DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        },
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, EnumWindows, FindWindowExW,
            GetClassNameW, GetClientRect, GetParent, GetWindowRect, GetWindowThreadProcessId,
            IsWindow, IsWindowVisible, RegisterClassW, UnregisterClassW, WNDCLASSW, WS_CHILD,
            WS_DISABLED, WS_EX_NOACTIVATE, WS_EX_NOPARENTNOTIFY,
        },
    },
};

use crate::{
    LifecycleReport, LifecycleState, ProbeError, Rect, TaskbarHost, TaskbarKind, WindowId,
    WindowSnapshot, lifecycle::valid_transition,
};

type GetDpi = unsafe extern "system" fn(HWND) -> u32;
type GetDpiContext = unsafe extern "system" fn(HWND) -> DPI_AWARENESS_CONTEXT;
type SetDpiContext = unsafe extern "system" fn(DPI_AWARENESS_CONTEXT) -> DPI_AWARENESS_CONTEXT;

#[derive(Clone, Copy)]
struct DpiApi {
    get_dpi: GetDpi,
    get_context: GetDpiContext,
    set_context: SetDpiContext,
}

impl DpiApi {
    fn load() -> Result<Self, ProbeError> {
        // SAFETY: The terminated module name is valid for the call. user32 is
        // already loaded by the statically imported window functions below.
        let module = unsafe { GetModuleHandleW(wide("user32.dll").as_ptr()) };
        if module.is_null() {
            return Err(last_error("GetModuleHandleW(user32.dll)"));
        }
        let resolve = |name: &'static [u8], diagnostic| {
            // SAFETY: These names are static, NUL-terminated ASCII exports and
            // module remains loaded throughout the process.
            unsafe { GetProcAddress(module, name.as_ptr()) }
                .ok_or(ProbeError::UnsupportedDpiApi(diagnostic))
        };
        let get_dpi = resolve(b"GetDpiForWindow\0", "GetDpiForWindow")?;
        let get_context = resolve(
            b"GetWindowDpiAwarenessContext\0",
            "GetWindowDpiAwarenessContext",
        )?;
        let set_context = resolve(
            b"SetThreadDpiAwarenessContext\0",
            "SetThreadDpiAwarenessContext",
        )?;
        // SAFETY: Each named user32 export has exactly the documented system
        // ABI and signature represented by the corresponding function alias.
        Ok(unsafe {
            Self {
                get_dpi: std::mem::transmute::<unsafe extern "system" fn() -> isize, GetDpi>(
                    get_dpi,
                ),
                get_context: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    GetDpiContext,
                >(get_context),
                set_context: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    SetDpiContext,
                >(set_context),
            }
        })
    }
}

/// Thread state must be restored on the same thread that changed it.
struct DpiScope {
    previous: Option<DPI_AWARENESS_CONTEXT>,
    setter: SetDpiContext,
    _thread_affinity: PhantomData<Rc<()>>,
}

impl DpiScope {
    fn enter(api: DpiApi, context: DPI_AWARENESS_CONTEXT) -> Result<Self, ProbeError> {
        // SAFETY: context comes from documented constants or GetWindowDpiAwarenessContext.
        let previous = unsafe { (api.set_context)(context) };
        if previous.is_null() {
            return Err(last_error("SetThreadDpiAwarenessContext"));
        }
        Ok(Self {
            previous: Some(previous),
            setter: api.set_context,
            _thread_affinity: PhantomData,
        })
    }

    fn physical_coordinates(api: DpiApi) -> Result<Self, ProbeError> {
        // V2 was introduced after V1; Windows 10 1607 can still use V1.
        Self::enter(api, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            .or_else(|_| Self::enter(api, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE))
    }

    fn restore(&mut self) -> Result<(), ProbeError> {
        if let Some(previous) = self.previous {
            // SAFETY: the prior context was returned for this thread; !Send and
            // !Sync prevent moving this guard to another thread.
            if unsafe { (self.setter)(previous) }.is_null() {
                return Err(last_error("restore thread DPI awareness"));
            }
            self.previous = None;
        }
        Ok(())
    }
}

impl Drop for DpiScope {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[derive(Default)]
struct Enumeration {
    hosts: Vec<(WindowId, TaskbarKind)>,
    resource_limit: bool,
}

impl Enumeration {
    fn finish(self) -> Result<Vec<(WindowId, TaskbarKind)>, ProbeError> {
        if self.resource_limit {
            Err(ProbeError::ResourceLimit)
        } else if self.hosts.is_empty() {
            Err(ProbeError::ExplorerUnavailable)
        } else {
            Ok(self.hosts)
        }
    }
}

// No panicking operations or unwinding are permitted across this callback ABI.
unsafe extern "system" fn enumerate(hwnd: HWND, data: LPARAM) -> i32 {
    // SAFETY: EnumWindows is synchronous; data points to the exclusively borrowed
    // Enumeration on the caller's stack for the entire enumeration.
    let state = unsafe { &mut *(data as *mut Enumeration) };
    let mut name = [0_u16; 256];
    // SAFETY: name is writable for the advertised 256 UTF-16 code units.
    let length = unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32) };
    if length <= 0 {
        return 1; // A concurrently destroyed unrelated window is irrelevant.
    }
    let name = &name[..length as usize];
    let kind = if name.iter().copied().eq("Shell_TrayWnd".encode_utf16()) {
        TaskbarKind::Primary
    } else if name
        .iter()
        .copied()
        .eq("Shell_SecondaryTrayWnd".encode_utf16())
    {
        TaskbarKind::Secondary
    } else {
        return 1;
    };
    if state.hosts.try_reserve(1).is_err() {
        state.resource_limit = true;
        return 0;
    }
    state.hosts.push((WindowId(hwnd as usize), kind));
    1
}

pub(crate) fn probe_taskbars() -> Result<Vec<TaskbarHost>, ProbeError> {
    let mut state = Enumeration::default();
    // SAFETY: EnumWindows invokes the system-ABI callback synchronously. The
    // pointer stays valid, exclusive, and correctly aligned throughout.
    let enumerated = unsafe {
        SetLastError(0);
        EnumWindows(Some(enumerate), (&mut state as *mut Enumeration) as LPARAM)
    };
    if state.resource_limit {
        return Err(ProbeError::ResourceLimit);
    }
    if enumerated == 0 {
        return Err(last_error("EnumWindows"));
    }
    let hosts = state.finish()?;
    let api = DpiApi::load()?;
    let mut dpi_scope = DpiScope::physical_coordinates(api)?;
    let result: Result<Vec<TaskbarHost>, ProbeError> = hosts
        .into_iter()
        .map(|(window, kind)| {
            let hwnd = window.0 as HWND;
            let taskbar = snapshot(hwnd, api)?;
            let expected = match kind {
                TaskbarKind::Primary => "Shell_TrayWnd",
                TaskbarKind::Secondary => "Shell_SecondaryTrayWnd",
            };
            if taskbar.class_name != expected {
                return Err(ProbeError::HostChanged);
            }
            let container =
                find_child(hwnd, "ReBarWindow32").or_else(|| find_child(hwnd, "WorkerW"));
            // Never call FindWindowEx with NULL parent when an optional container is
            // absent: that would accidentally search unrelated top-level windows.
            let buttons = container.and_then(|parent| {
                find_child(parent, "MSTaskSwWClass")
                    .or_else(|| find_child(parent, "MSTaskListWClass"))
            });
            Ok(TaskbarHost {
                kind,
                taskbar,
                classical_container: container.map(|window| snapshot(window, api)).transpose()?,
                task_buttons: buttons.map(|window| snapshot(window, api)).transpose()?,
                notification_area: find_child(hwnd, "TrayNotifyWnd")
                    .map(|window| snapshot(window, api))
                    .transpose()?,
                start_button: find_child(hwnd, "Start")
                    .map(|window| snapshot(window, api))
                    .transpose()?,
            })
        })
        .collect();
    let restored = dpi_scope.restore();
    result.and_then(|hosts| restored.map(|()| hosts))
}

fn find_child(parent: HWND, class_name: &str) -> Option<HWND> {
    // SAFETY: the terminated class name remains valid; parent came from current
    // enumeration. A concurrently destroyed parent produces NULL, not ownership.
    let found = unsafe {
        FindWindowExW(
            parent,
            null_mut(),
            wide(class_name).as_ptr(),
            std::ptr::null(),
        )
    };
    (!found.is_null()).then_some(found)
}

fn snapshot(hwnd: HWND, api: DpiApi) -> Result<WindowSnapshot, ProbeError> {
    let mut name = [0_u16; 256];
    let mut screen = RECT::default();
    let mut client = RECT::default();
    let mut process_id = 0;
    // SAFETY: output buffers are correctly sized, writable locals. The APIs
    // validate the borrowed HWND; none retains pointers or owns Explorer handles.
    let (length, thread_id) = unsafe {
        let length = GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32);
        if length == 0 {
            return Err(last_error("GetClassNameW"));
        }
        if GetWindowRect(hwnd, &mut screen) == 0 {
            return Err(last_error("GetWindowRect"));
        }
        if GetClientRect(hwnd, &mut client) == 0 {
            return Err(last_error("GetClientRect"));
        }
        let thread_id = GetWindowThreadProcessId(hwnd, &mut process_id);
        if thread_id == 0 {
            return Err(last_error("GetWindowThreadProcessId"));
        }
        (length as usize, thread_id)
    };
    let mut top_left = POINT {
        x: client.left,
        y: client.top,
    };
    let mut bottom_right = POINT {
        x: client.right,
        y: client.bottom,
    };
    // SAFETY: both points are writable locals and hwnd is a borrowed window.
    unsafe {
        if ClientToScreen(hwnd, &mut top_left) == 0 {
            return Err(last_error("ClientToScreen(top-left)"));
        }
        if ClientToScreen(hwnd, &mut bottom_right) == 0 {
            return Err(last_error("ClientToScreen(bottom-right)"));
        }
    }
    // SAFETY: the dynamically resolved API has the documented HWND signature.
    let dpi = unsafe { (api.get_dpi)(hwnd) };
    if dpi == 0 {
        return Err(ProbeError::HostChanged); // GetDpiForWindow does not define last-error.
    }
    Ok(WindowSnapshot {
        window: WindowId(hwnd as usize),
        class_name: String::from_utf16_lossy(&name[..length]),
        process_id,
        thread_id,
        screen_rect: rect(screen),
        client_rect: rect(client),
        client_screen_rect: screen_bounds(top_left, bottom_right),
        dpi,
    })
}

static CLASS_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Owns only this process's class/window, and can never cross a thread boundary.
struct HiddenChild {
    class_name: Vec<u16>,
    instance: HINSTANCE,
    owner_thread: u32,
    hwnd: Option<HWND>,
    class_registered: bool,
    states: Vec<LifecycleState>,
    _thread_affinity: PhantomData<Rc<()>>,
}

impl HiddenChild {
    fn create(parent: HWND) -> Result<Self, ProbeError> {
        // SAFETY: NULL requests the current process's module; ID getters have no preconditions.
        let (instance, process_id, owner_thread) = unsafe {
            (
                GetModuleHandleW(std::ptr::null()),
                GetCurrentProcessId(),
                GetCurrentThreadId(),
            )
        };
        if instance.is_null() {
            return Err(last_error("GetModuleHandleW(current process)"));
        }
        let class_name = wide(&format!(
            "TrafficMonitor.Rust.HiddenProbe.{process_id}.{owner_thread}.{}",
            CLASS_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            ..WNDCLASSW::default()
        };
        // SAFETY: the class structure and terminated name remain valid. The
        // stateless callback has system ABI and stays in this loaded module.
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(last_error("RegisterClassW(hidden probe)"));
        }
        let mut child = Self {
            class_name,
            instance,
            owner_thread,
            hwnd: None,
            class_registered: true,
            states: vec![LifecycleState::ClassRegistered],
            _thread_affinity: PhantomData,
        };
        // SAFETY: class and parent are borrowed valid handles. No WS_VISIBLE is
        // requested, no lpParam is retained, and this thread owns the new child.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_NOPARENTNOTIFY,
                child.class_name.as_ptr(),
                wide("TrafficMonitor hidden lifecycle probe").as_ptr(),
                WS_CHILD | WS_DISABLED,
                0,
                0,
                1,
                1,
                parent,
                null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err(last_error("CreateWindowExW(hidden child)"));
        }
        child.hwnd = Some(hwnd);
        child.advance(LifecycleState::ChildCreated)?;
        Ok(child)
    }

    fn advance(&mut self, next: LifecycleState) -> Result<(), ProbeError> {
        if !self
            .states
            .last()
            .is_some_and(|from| valid_transition(*from, next))
        {
            return Err(ProbeError::UnexpectedChildState(
                "invalid lifecycle transition",
            ));
        }
        self.states.push(next);
        Ok(())
    }

    fn close(&mut self) -> Result<(), ProbeError> {
        // SAFETY: GetCurrentThreadId has no preconditions. !Send/!Sync enforce this invariant.
        if unsafe { GetCurrentThreadId() } != self.owner_thread {
            return Err(ProbeError::UnexpectedChildState(
                "resource cleanup on another thread",
            ));
        }
        if let Some(hwnd) = self.hwnd {
            // SAFETY: this guard exclusively owns the child created on this
            // thread. Parent teardown may already have destroyed it; do not
            // mistake that for ownership of an Explorer window.
            if unsafe { IsWindow(hwnd) } != 0 && unsafe { DestroyWindow(hwnd) } == 0 {
                return Err(last_error("DestroyWindow(hidden child)"));
            }
            self.hwnd = None;
            self.advance(LifecycleState::ChildDestroyed)?;
        }
        if self.class_registered {
            // SAFETY: this private registered class belongs to this module and
            // its only child has been destroyed before attempting unregister.
            if unsafe { UnregisterClassW(self.class_name.as_ptr(), self.instance) } == 0 {
                return Err(last_error("UnregisterClassW(hidden probe)"));
            }
            self.class_registered = false;
            self.advance(LifecycleState::ClassUnregistered)?;
        }
        Ok(())
    }
}

impl Drop for HiddenChild {
    fn drop(&mut self) {
        // Explicit close reports errors on the normal path; Drop also cleans up
        // every early return without panicking during unwinding.
        let _ = self.close();
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: all messages are forwarded unchanged to Windows' default procedure;
    // the callback holds no Rust reference and cannot unwind into Windows.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub(crate) fn probe_hidden_child(host: &TaskbarHost) -> Result<LifecycleReport, ProbeError> {
    let api = DpiApi::load()?;
    let parent = host.taskbar.window.0 as HWND;
    let current = snapshot(parent, api)?;
    let expected_class = match host.kind {
        TaskbarKind::Primary => "Shell_TrayWnd",
        TaskbarKind::Secondary => "Shell_SecondaryTrayWnd",
    };
    if current.class_name != expected_class
        || current.class_name != host.taskbar.class_name
        || current.process_id != host.taskbar.process_id
        || current.thread_id != host.taskbar.thread_id
    {
        return Err(ProbeError::HostChanged);
    }
    // Match the foreign parent's DPI awareness to avoid cross-awareness
    // CreateWindowEx failures. This changes only this thread and is restored.
    // SAFETY: parent was just checked as an existing window.
    let context = unsafe { (api.get_context)(parent) };
    if context.is_null() {
        return Err(ProbeError::HostChanged);
    }
    let mut dpi_scope = DpiScope::enter(api, context)?;
    let result = (|| {
        let mut child = HiddenChild::create(parent)?;
        let hwnd = child
            .hwnd
            .ok_or(ProbeError::UnexpectedChildState("missing created child"))?;
        // SAFETY: the guard owns hwnd; queries do not change parent or visibility.
        unsafe {
            if GetParent(hwnd) != parent {
                return Err(ProbeError::UnexpectedChildState(
                    "parent does not match taskbar",
                ));
            }
            if IsWindowVisible(hwnd) != 0 {
                return Err(ProbeError::UnexpectedChildState(
                    "child unexpectedly visible",
                ));
            }
        }
        child.advance(LifecycleState::HiddenChildVerified)?;
        child.close()?;
        Ok(LifecycleReport {
            child_window: WindowId(hwnd as usize),
            parent_window: host.taskbar.window,
            states: child.states.clone(),
        })
    })();
    let restored = dpi_scope.restore();
    result.and_then(|report| restored.map(|()| report))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn rect(value: RECT) -> Rect {
    Rect {
        left: value.left,
        top: value.top,
        right: value.right,
        bottom: value.bottom,
    }
}

fn screen_bounds(first: POINT, second: POINT) -> Rect {
    // Mirrored (WS_EX_LAYOUTRTL) windows can map client-left to screen-right.
    // The snapshot describes physical bounds, independent of the point order.
    Rect {
        left: first.x.min(second.x),
        top: first.y.min(second.y),
        right: first.x.max(second.x),
        bottom: first.y.max(second.y),
    }
}

fn last_error(operation: &'static str) -> ProbeError {
    // SAFETY: called immediately after the failing Win32 API, on the same thread.
    ProbeError::Win32 {
        operation,
        code: unsafe { GetLastError() },
    }
}

#[cfg(test)]
mod tests {
    use super::{Enumeration, POINT, ProbeError, Rect, TaskbarKind, WindowId, screen_bounds};

    #[test]
    fn rtl_mapped_client_corners_have_noninverted_screen_bounds() {
        // WS_EX_LAYOUTRTL can map client-left to the screen's right edge.
        assert_eq!(
            screen_bounds(POINT { x: 100, y: 40 }, POINT { x: -100, y: 88 }),
            Rect {
                left: -100,
                top: 40,
                right: 100,
                bottom: 88
            }
        );
        // Point order must not affect the physical bounding rectangle.
        assert_eq!(
            screen_bounds(POINT { x: -100, y: 88 }, POINT { x: 100, y: 40 }),
            Rect {
                left: -100,
                top: 40,
                right: 100,
                bottom: 88
            }
        );
    }

    #[test]
    fn screen_bounds_preserve_negative_and_extreme_coordinates() {
        assert_eq!(
            screen_bounds(
                POINT {
                    x: i32::MIN,
                    y: -1440
                },
                POINT {
                    x: i32::MAX,
                    y: -1392
                }
            ),
            Rect {
                left: i32::MIN,
                top: -1440,
                right: i32::MAX,
                bottom: -1392
            }
        );
    }

    #[test]
    fn screen_bounds_preserve_empty_client_dimensions() {
        assert_eq!(
            screen_bounds(POINT { x: -20, y: 50 }, POINT { x: -20, y: 50 }),
            Rect {
                left: -20,
                top: 50,
                right: -20,
                bottom: 50
            }
        );
    }

    #[test]
    fn absent_explorer_is_an_explicit_diagnostic() {
        assert_eq!(
            Enumeration::default().finish(),
            Err(ProbeError::ExplorerUnavailable)
        );
    }

    #[test]
    fn resource_failure_never_returns_an_incomplete_host_list() {
        let state = Enumeration {
            hosts: vec![(WindowId(1), TaskbarKind::Primary)],
            resource_limit: true,
        };
        assert_eq!(state.finish(), Err(ProbeError::ResourceLimit));
    }

    #[test]
    fn both_primary_and_secondary_observations_are_preserved() {
        let hosts = vec![
            (WindowId(1), TaskbarKind::Primary),
            (WindowId(2), TaskbarKind::Secondary),
        ];
        let state = Enumeration {
            hosts: hosts.clone(),
            resource_limit: false,
        };
        assert_eq!(state.finish(), Ok(hosts));
    }
}
