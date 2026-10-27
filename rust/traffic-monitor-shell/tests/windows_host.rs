//! Explicit interactive acceptance probes. Never run during the default suite.
#![cfg(windows)]

use traffic_monitor_shell::{LifecycleState, probe_hidden_child, probe_taskbars};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        HiDpi::{AreDpiAwarenessContextsEqual, GetThreadDpiAwarenessContext},
        WindowsAndMessaging::IsWindow,
    },
};

#[test]
#[ignore = "requires an interactive Explorer desktop; read-only real-host acceptance"]
fn readonly_host_probe_restores_the_callers_dpi_context() {
    // SAFETY: these APIs query only the current thread's DPI state.
    let before = unsafe { GetThreadDpiAwarenessContext() };
    let hosts = probe_taskbars().expect("interactive taskbar is required for this explicit test");
    assert!(!hosts.is_empty());
    for host in hosts {
        assert!(host.taskbar.dpi > 0);
        assert!(host.taskbar.client_rect.right >= host.taskbar.client_rect.left);
        assert!(host.taskbar.client_rect.bottom >= host.taskbar.client_rect.top);
        let client_width =
            i64::from(host.taskbar.client_rect.right) - i64::from(host.taskbar.client_rect.left);
        let screen_width = i64::from(host.taskbar.client_screen_rect.right)
            - i64::from(host.taskbar.client_screen_rect.left);
        assert_eq!(client_width, screen_width);
    }
    // SAFETY: valid contexts come from the current thread; comparison is read-only.
    assert_ne!(
        unsafe { AreDpiAwarenessContextsEqual(before, GetThreadDpiAwarenessContext()) },
        0
    );
}

#[test]
#[ignore = "explicitly creates/destroys hidden child windows on an interactive Explorer taskbar"]
fn hidden_children_are_destroyed_and_thread_dpi_is_restored() {
    // SAFETY: read-only thread DPI query.
    let before = unsafe { GetThreadDpiAwarenessContext() };
    let hosts = probe_taskbars().expect("interactive taskbar is required for this explicit test");
    for host in hosts {
        // Repetition checks release of the window class as well as each window.
        for _ in 0..8 {
            let report = probe_hidden_child(&host).expect("hidden child lifecycle");
            assert_eq!(report.parent_window, host.taskbar.window);
            assert_eq!(
                report.states,
                [
                    LifecycleState::ClassRegistered,
                    LifecycleState::ChildCreated,
                    LifecycleState::HiddenChildVerified,
                    LifecycleState::ChildDestroyed,
                    LifecycleState::ClassUnregistered,
                ]
            );
            // SAFETY: IsWindow accepts an already-destroyed diagnostic handle.
            assert_eq!(unsafe { IsWindow(report.child_window.value() as HWND) }, 0);
        }
    }
    // SAFETY: valid contexts come from the same thread; comparison is read-only.
    assert_ne!(
        unsafe { AreDpiAwarenessContextsEqual(before, GetThreadDpiAwarenessContext()) },
        0
    );
}
