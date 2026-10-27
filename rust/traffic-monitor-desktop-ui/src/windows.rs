#![allow(unsafe_op_in_unsafe_fn)]
use crate::{layout::*, settings_tabs::*, *};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    ffi::c_void,
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::{null, null_mut},
    rc::Rc,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Gdi::*, GdiPlus::*},
    System::{LibraryLoader::*, Registry::*},
    UI::{
        Controls::Dialogs::*, Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*,
        WindowsAndMessaging::*,
    },
};
const CLASS: &str = "TrafficMonitor.Rust.Desktop";
// Win32 static-control styles live behind the unrelated SystemServices feature.
const SS_LEFTNOWORDWRAP: u32 = 0x000c;
const SS_ENDELLIPSIS: u32 = 0x4000;
const TRAY: u32 = WM_APP + 1;
const REFRESH: u32 = WM_APP + 2;
use std::result::Result::Ok;
thread_local! {static APP:RefCell<Option<Rc<App>>>=const{RefCell::new(None)};}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn current() -> Option<Rc<App>> {
    APP.with(|v| v.borrow().clone())
}
unsafe fn window_dpi(hwnd: HWND) -> u32 {
    let module = GetModuleHandleW(windows_sys::core::w!("user32.dll"));
    if let Some(function) = GetProcAddress(module, c"GetDpiForWindow".as_ptr().cast()) {
        let function: unsafe extern "system" fn(HWND) -> u32 = std::mem::transmute(function);
        let dpi = function(hwnd);
        if dpi != 0 {
            return dpi;
        }
    }
    let dc = GetDC(hwnd);
    if dc.is_null() {
        return 96;
    }
    let dpi = GetDeviceCaps(dc, LOGPIXELSX as i32);
    ReleaseDC(hwnd, dc);
    dpi.max(96) as u32
}
unsafe fn set_thread_dpi(context: DPI_AWARENESS_CONTEXT) -> DPI_AWARENESS_CONTEXT {
    let module = GetModuleHandleW(windows_sys::core::w!("user32.dll"));
    if let Some(function) = GetProcAddress(module, c"SetThreadDpiAwarenessContext".as_ptr().cast())
    {
        let function: unsafe extern "system" fn(DPI_AWARENESS_CONTEXT) -> DPI_AWARENESS_CONTEXT =
            std::mem::transmute(function);
        let previous = function(context);
        if previous.is_null() && context == DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 {
            return function(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE);
        }
        return previous;
    }
    SetProcessDPIAware();
    null_mut()
}
#[derive(Clone, Copy)]
struct ObservedId(usize);
impl ObservedId {
    fn value(self) -> usize {
        self.0
    }
}
struct ObservedWindow {
    window: ObservedId,
    screen_rect: traffic_monitor_shell::Rect,
    client_screen_rect: traffic_monitor_shell::Rect,
}
struct DesktopHost {
    taskbar: ObservedWindow,
    task_buttons: Option<ObservedWindow>,
    notification_area: Option<ObservedWindow>,
    start_button: Option<ObservedWindow>,
}
struct WindowDpiScope {
    previous: DPI_AWARENESS_CONTEXT,
}
impl WindowDpiScope {
    unsafe fn enter(hwnd: HWND) -> Self {
        let module = GetModuleHandleW(windows_sys::core::w!("user32.dll"));
        let previous = if let Some(function) =
            GetProcAddress(module, c"GetWindowDpiAwarenessContext".as_ptr().cast())
        {
            let function: unsafe extern "system" fn(HWND) -> DPI_AWARENESS_CONTEXT =
                std::mem::transmute(function);
            let context = function(hwnd);
            if context.is_null() {
                null_mut()
            } else {
                set_thread_dpi(context)
            }
        } else {
            null_mut()
        };
        Self { previous }
    }
}
impl Drop for WindowDpiScope {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.is_null() {
                set_thread_dpi(self.previous);
            }
        }
    }
}
struct TextMode {
    font: String,
    points: i32,
    style: u8,
    dpi: u32,
    directwrite: bool,
    colorful: bool,
}
impl From<traffic_monitor_shell::WindowSnapshot> for ObservedWindow {
    fn from(w: traffic_monitor_shell::WindowSnapshot) -> Self {
        Self {
            window: ObservedId(w.window.value()),
            screen_rect: w.screen_rect,
            client_screen_rect: w.client_screen_rect,
        }
    }
}
unsafe fn probe_desktop_taskbars() -> Result<Vec<DesktopHost>, String> {
    if let Ok(hosts) = traffic_monitor_shell::probe_taskbars() {
        return Ok(hosts
            .into_iter()
            .map(|h| DesktopHost {
                taskbar: h.taskbar.into(),
                task_buttons: h.task_buttons.map(Into::into),
                notification_area: h.notification_area.map(Into::into),
                start_button: h.start_button.map(Into::into),
            })
            .collect());
    }
    unsafe fn observed(hwnd: HWND) -> Option<ObservedWindow> {
        if hwnd.is_null() || IsWindow(hwnd) == 0 {
            return None;
        }
        let mut r: RECT = zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 {
            return None;
        }
        let screen_rect = traffic_monitor_shell::Rect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        };
        GetClientRect(hwnd, &mut r);
        MapWindowPoints(hwnd, null_mut(), &mut r as *mut RECT as *mut POINT, 2);
        Some(ObservedWindow {
            window: ObservedId(hwnd as usize),
            screen_rect,
            client_screen_rect: traffic_monitor_shell::Rect {
                left: r.left.min(r.right),
                top: r.top.min(r.bottom),
                right: r.left.max(r.right),
                bottom: r.top.max(r.bottom),
            },
        })
    }
    let primary = FindWindowW(windows_sys::core::w!("Shell_TrayWnd"), null());
    let mut windows = vec![];
    if !primary.is_null() {
        windows.push(primary);
    }
    let mut after = null_mut();
    for _ in 0..64 {
        after = FindWindowExW(
            null_mut(),
            after,
            windows_sys::core::w!("Shell_SecondaryTrayWnd"),
            null(),
        );
        if after.is_null() {
            break;
        }
        windows.push(after);
    }
    let mut hosts = vec![];
    for hwnd in windows {
        let Some(taskbar) = observed(hwnd) else {
            continue;
        };
        let mut container = FindWindowExW(
            hwnd,
            null_mut(),
            windows_sys::core::w!("ReBarWindow32"),
            null(),
        );
        if container.is_null() {
            container = FindWindowExW(hwnd, null_mut(), windows_sys::core::w!("WorkerW"), null());
        }
        if container.is_null() {
            container = hwnd;
        }
        let mut buttons = FindWindowExW(
            container,
            null_mut(),
            windows_sys::core::w!("MSTaskSwWClass"),
            null(),
        );
        if buttons.is_null() {
            buttons = FindWindowExW(
                container,
                null_mut(),
                windows_sys::core::w!("MSTaskListWClass"),
                null(),
            );
        }
        hosts.push(DesktopHost {
            taskbar,
            task_buttons: observed(buttons),
            notification_area: observed(FindWindowExW(
                hwnd,
                null_mut(),
                windows_sys::core::w!("TrayNotifyWnd"),
                null(),
            )),
            start_button: observed(FindWindowExW(
                hwnd,
                null_mut(),
                windows_sys::core::w!("Start"),
                null(),
            )),
        });
    }
    if hosts.is_empty() {
        Err("Explorer taskbar is temporarily unavailable".into())
    } else {
        Ok(hosts)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Settings,
    Advanced,
    Taskbar,
    History,
    Network,
    Appearance,
    Plugins,
}
struct Panel {
    hwnd: HWND,
    page: Page,
    fields: Vec<(String, HWND, SettingKind)>,
    list: HWND,
    detail: HWND,
    aux: Vec<HWND>,
    settings: Option<SettingsPanel>,
}
struct ItemPicker {
    list: HWND,
    entries: Vec<(String, String)>,
}
struct SettingsPanel {
    tab: HWND,
    pages: Vec<(HWND, i32)>,
    baseline: Vec<(String, String)>,
    pending: Option<Vec<(String, String)>>,
    close_after_apply: bool,
    main_items: ItemPicker,
    taskbar_items: ItemPicker,
    appearance: (HWND, HWND),
}
unsafe extern "system" fn settings_page_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if matches!(msg, WM_COMMAND | WM_NOTIFY | WM_VSCROLL | WM_HSCROLL) {
        if let Some(app) = current() {
            return app.message(hwnd, msg, w, l);
        }
    }
    if msg == WM_MOUSEWHEEL {
        if let Some(app) = current() {
            let direction = if (w >> 16) as i16 > 0 {
                SB_LINEUP
            } else {
                SB_LINEDOWN
            };
            for _ in 0..3 {
                app.message(hwnd, WM_VSCROLL, direction as usize, 0);
            }
            return 0;
        }
    }
    DefSubclassProc(hwnd, msg, w, l)
}
struct ListEditor {
    hwnd: HWND,
    target: HWND,
    parent: HWND,
    list: HWND,
    entry: HWND,
}
struct PaintSession {
    window: HWND,
    paint: PAINTSTRUCT,
    dc: HDC,
}
impl PaintSession {
    unsafe fn begin(window: HWND) -> Self {
        let mut paint = zeroed();
        let dc = BeginPaint(window, &mut paint);
        Self { window, paint, dc }
    }
}
impl Drop for PaintSession {
    fn drop(&mut self) {
        unsafe {
            EndPaint(self.window, &self.paint);
        }
    }
}
struct DcState {
    dc: HDC,
    saved: i32,
}
struct Tooltip {
    hwnd: HWND,
    text: Vec<u16>,
    target: HWND,
}
/// Owns a top-down premultiplied ARGB surface used by UpdateLayeredWindow.
struct DibSurface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u32,
    width: i32,
    height: i32,
}
impl DibSurface {
    unsafe fn text(&self, hdc: HDC, text: &str, rect: &RECT, flags: u32, color: u32) {
        let text = wide(text);
        let mut target = null_mut();
        let mut graphics = null_mut();
        let mut font = null_mut();
        let mut brush = null_mut();
        let mut format = null_mut();
        if GdipCreateBitmapFromScan0(
            self.width,
            self.height,
            self.width * 4,
            0x000e200b,
            self.bits.cast(),
            &mut target,
        ) == 0
            && GdipGetImageGraphicsContext(target.cast(), &mut graphics) == 0
            && GdipCreateFontFromDC(hdc, &mut font) == 0
            && GdipCreateSolidFill(
                0xff000000 | ((color & 255) << 16) | (color & 0xff00) | ((color >> 16) & 255),
                &mut brush,
            ) == 0
            && GdipStringFormatGetGenericTypographic(&mut format) == 0
        {
            GdipSetTextRenderingHint(graphics, TextRenderingHintAntiAliasGridFit);
            GdipSetStringFormatFlags(format, StringFormatFlagsNoWrap);
            GdipSetStringFormatTrimming(format, StringTrimmingEllipsisCharacter);
            GdipSetStringFormatAlign(
                format,
                if flags & DT_RIGHT != 0 {
                    StringAlignmentFar
                } else if flags & DT_CENTER != 0 {
                    StringAlignmentCenter
                } else {
                    StringAlignmentNear
                },
            );
            GdipSetStringFormatLineAlign(format, StringAlignmentCenter);
            let r = RectF {
                X: rect.left as f32,
                Y: rect.top as f32,
                Width: (rect.right - rect.left) as f32,
                Height: (rect.bottom - rect.top) as f32,
            };
            GdipDrawString(graphics, text.as_ptr(), -1, font, &r, format, brush.cast());
        }
        if !format.is_null() {
            GdipDeleteStringFormat(format);
        }
        if !brush.is_null() {
            GdipDeleteBrush(brush.cast());
        }
        if !font.is_null() {
            GdipDeleteFont(font);
        }
        if !graphics.is_null() {
            GdipDeleteGraphics(graphics);
        }
        if !target.is_null() {
            GdipDisposeImage(target.cast());
        }
    }
    unsafe fn new(reference: HDC, width: i32, height: i32) -> Option<Self> {
        if width <= 0 || height <= 0 || width > 8192 || height > 8192 {
            return None;
        }
        let dc = CreateCompatibleDC(reference);
        if dc.is_null() {
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..zeroed()
            },
            ..zeroed()
        };
        let mut bits = null_mut();
        let bitmap = CreateDIBSection(reference, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bitmap.is_null() {
            DeleteDC(dc);
            return None;
        }
        let previous = SelectObject(dc, bitmap);
        std::ptr::write_bytes(bits, 0, width as usize * height as usize * 4);
        Some(Self {
            dc,
            bitmap,
            previous,
            bits: bits.cast(),
            width,
            height,
        })
    }
    unsafe fn pixels(&self) -> &[u32] {
        GdiFlush();
        std::slice::from_raw_parts(self.bits, self.width as usize * self.height as usize)
    }
    unsafe fn draw_background(&self, source: *mut GpBitmap) {
        let mut target = null_mut();
        // GDI+ PixelFormat32bppPARGB = 11 | (32 << 8) | Alpha | PAlpha | GDI.
        if GdipCreateBitmapFromScan0(
            self.width,
            self.height,
            self.width * 4,
            0x000e200b,
            self.bits.cast(),
            &mut target,
        ) == 0
        {
            let mut graphics = null_mut();
            if GdipGetImageGraphicsContext(target.cast(), &mut graphics) == 0 {
                GdipDrawImageRectI(graphics, source.cast(), 0, 0, self.width, self.height);
                GdipDeleteGraphics(graphics);
            }
            GdipDisposeImage(target.cast());
        }
    }
    unsafe fn present(&self, hwnd: HWND, opacity: u8, before: &[u32]) {
        GdiFlush();
        let pixels =
            std::slice::from_raw_parts_mut(self.bits, self.width as usize * self.height as usize);
        for (pixel, old) in pixels.iter_mut().zip(before) {
            if *pixel != *old && (*pixel >> 24) == 0 {
                *pixel |= 0xff000000;
            }
        }
        let mut r = zeroed();
        GetWindowRect(hwnd, &mut r);
        let position = POINT {
            x: r.left,
            y: r.top,
        };
        let size = SIZE {
            cx: self.width,
            cy: self.height,
        };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: opacity,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        UpdateLayeredWindow(
            hwnd,
            null_mut(),
            &position,
            &size,
            self.dc,
            &source,
            0,
            &blend,
            ULW_ALPHA,
        );
    }
}
impl Drop for DibSurface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
impl Drop for Tooltip {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.hwnd) != 0 {
                DestroyWindow(self.hwnd);
            }
        }
    }
}
impl DcState {
    unsafe fn save(dc: HDC) -> Self {
        Self {
            dc,
            saved: SaveDC(dc),
        }
    }
}
impl Drop for DcState {
    fn drop(&mut self) {
        unsafe {
            if self.saved != 0 {
                RestoreDC(self.dc, self.saved);
            }
        }
    }
}
struct Taskbar {
    hwnd: HWND,
    parent: HWND,
    displaced: Option<(HWND, RECT, RECT)>,
    parent_size: (i32, i32),
    tooltip: HWND,
    tooltip_text: Vec<u16>,
}
impl Drop for Taskbar {
    fn drop(&mut self) {
        unsafe {
            if !self.tooltip.is_null() && IsWindow(self.tooltip) != 0 {
                DestroyWindow(self.tooltip);
            }
            if IsWindow(self.hwnd) != 0 {
                DestroyWindow(self.hwnd);
            }
            if let Some((hwnd, original, expected)) = self.displaced {
                if IsWindow(hwnd) != 0 {
                    let mut r = zeroed();
                    GetWindowRect(hwnd, &mut r);
                    MapWindowPoints(
                        null_mut(),
                        GetParent(hwnd),
                        &mut r as *mut RECT as *mut POINT,
                        2,
                    );
                    if r.left == expected.left
                        && r.top == expected.top
                        && r.right == expected.right
                        && r.bottom == expected.bottom
                    {
                        MoveWindow(
                            hwnd,
                            original.left,
                            original.top,
                            original.right - original.left,
                            original.bottom - original.top,
                            1,
                        );
                    }
                }
            }
        }
    }
}
struct App {
    backend: RefCell<Box<dyn DesktopBackend>>,
    options: RefCell<DesktopOptions>,
    dashboard: RefCell<Dashboard>,
    main: Cell<HWND>,
    taskbar: RefCell<Option<Taskbar>>,
    panel: RefCell<Option<Panel>>,
    actions: RefCell<VecDeque<DesktopAction>>,
    taskbar_created: u32,
    taskbar_generation: Cell<usize>,
    taskbar_desired_width: Cell<i32>,
    taskbar_item_rects: RefCell<Vec<(String, [i32; 4])>>,
    font: Cell<HFONT>,
    taskbar_font: Cell<HFONT>,
    panel_font: Cell<HFONT>,
    theme_background: Cell<u32>,
    theme_foreground: Cell<u32>,
    tray: Cell<bool>,
    icon: Cell<HICON>,
    icon_path: RefCell<PathBuf>,
    gdiplus: usize,
    skin: Cell<*mut GpBitmap>,
    skin_path: RefCell<Option<PathBuf>>,
    quitting: Cell<bool>,
    active_plugin: RefCell<Option<String>>,
    list_editor: RefCell<Option<ListEditor>>,
    main_tooltip: RefCell<Option<Tooltip>>,
    renderer_warning: Cell<bool>,
}
impl Drop for App {
    fn drop(&mut self) {
        unsafe {
            self.remove_tray();
            self.main_tooltip.get_mut().take();
            if let Some(editor) = self.list_editor.get_mut().take() {
                if IsWindow(editor.hwnd) != 0 {
                    DestroyWindow(editor.hwnd);
                }
            }
            self.taskbar.get_mut().take();
            if let Some(p) = self.panel.get_mut().take() {
                if IsWindow(p.hwnd) != 0 {
                    DestroyWindow(p.hwnd);
                }
            }
            if !self.font.get().is_null() {
                DeleteObject(self.font.get());
            }
            if !self.taskbar_font.get().is_null() {
                DeleteObject(self.taskbar_font.get());
            }
            if !self.panel_font.get().is_null() {
                DeleteObject(self.panel_font.get());
            }
            if !self.skin.get().is_null() {
                GdipDisposeImage(self.skin.get().cast());
            }
            if self.gdiplus != 0 {
                GdiplusShutdown(self.gdiplus);
            }
            if !self.icon.get().is_null() {
                DestroyIcon(self.icon.get());
            }
        }
    }
}
pub fn run(backend: Box<dyn DesktopBackend>) -> Result<(), String> {
    unsafe {
        let options = backend.options();
        validate_options(&options)?;
        if APP.with(|a| a.borrow().is_some()) {
            return Err("Desktop event loop already active on this thread".into());
        }
        let old_dpi = set_thread_dpi(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let controls = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_WIN95_CLASSES | ICC_DATE_CLASSES,
        };
        InitCommonControlsEx(&controls);
        let instance = GetModuleHandleW(null());
        let name = wide(CLASS);
        let wc = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: name.as_ptr(),
            hIcon: LoadIconW(instance, 1usize as _),
            ..zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            if !old_dpi.is_null() {
                set_thread_dpi(old_dpi);
            }
            return Err(format!("RegisterClassW: {}", GetLastError()));
        }
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token = 0;
        GdiplusStartup(&mut token, &input, null_mut());
        let icon_path = options.tray_icon.clone().unwrap_or_else(|| {
            [
                "TrafficMonitor.ico",
                "res/TrafficMonitor.ico",
                "TrafficMonitor/res/TrafficMonitor.ico",
            ]
            .iter()
            .map(|p| options.resource_dir.join(p))
            .find(|p| p.is_file())
            .unwrap_or_else(|| options.resource_dir.join("TrafficMonitor.ico"))
        });
        let mut icon = LoadImageW(
            null_mut(),
            wide(&icon_path.to_string_lossy()).as_ptr(),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE | LR_DEFAULTSIZE,
        ) as HICON;
        if icon.is_null() {
            let resource = LoadIconW(instance, 1usize as _);
            icon = CopyIcon(if resource.is_null() {
                LoadIconW(null_mut(), IDI_APPLICATION)
            } else {
                resource
            });
        }
        let app = Rc::new(App {
            backend: RefCell::new(backend),
            options: RefCell::new(options.clone()),
            dashboard: RefCell::new(Dashboard::default()),
            main: Cell::new(null_mut()),
            taskbar: RefCell::new(None),
            panel: RefCell::new(None),
            actions: RefCell::new(VecDeque::new()),
            taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            taskbar_generation: Cell::new(0),
            taskbar_desired_width: Cell::new(0),
            taskbar_item_rects: RefCell::new(vec![]),
            font: Cell::new(null_mut()),
            taskbar_font: Cell::new(null_mut()),
            panel_font: Cell::new(null_mut()),
            theme_background: Cell::new(options.background),
            theme_foreground: Cell::new(options.foreground),
            tray: Cell::new(false),
            icon: Cell::new(icon),
            icon_path: RefCell::new(icon_path),
            gdiplus: token,
            skin: Cell::new(null_mut()),
            skin_path: RefCell::new(None),
            quitting: Cell::new(false),
            active_plugin: RefCell::new(None),
            list_editor: RefCell::new(None),
            main_tooltip: RefCell::new(None),
            renderer_warning: Cell::new(false),
        });
        APP.with(|a| *a.borrow_mut() = Some(app.clone()));
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            name.as_ptr(),
            wide(&options.title).as_ptr(),
            WS_POPUP,
            options.x,
            options.y,
            options.width,
            options.height,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            APP.with(|a| a.borrow_mut().take());
            UnregisterClassW(name.as_ptr(), instance);
            if !old_dpi.is_null() {
                set_thread_dpi(old_dpi);
            }
            return Err(format!("CreateWindowExW: {}", GetLastError()));
        }
        app.main.set(hwnd);
        SendMessageW(
            hwnd,
            WM_SETICON,
            ICON_SMALL as usize,
            app.icon.get() as isize,
        );
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, app.icon.get() as isize);
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            app.apply_window_options();
            app.refresh();
        }))
        .is_err()
        {
            PostQuitMessage(1);
        }
        SetTimer(hwnd, 1, options.interval_ms, None);
        if let Some(ms) = options.exit_after_ms {
            SetTimer(hwnd, 2, ms.max(100), None);
        }
        let mut msg = zeroed();
        let mut failure = None;
        loop {
            let result = GetMessageW(&mut msg, null_mut(), 0, 0);
            if result <= 0 {
                if result < 0 {
                    failure = Some(format!("GetMessageW: {}", GetLastError()));
                } else if msg.wParam != 0 {
                    failure = Some(
                        "Desktop window callback panicked; the message loop was shut down safely"
                            .into(),
                    );
                }
                break;
            }
            let editor = app.list_editor.borrow().as_ref().map(|p| p.hwnd);
            let panel = editor.or_else(|| app.panel.borrow().as_ref().map(|p| p.hwnd));
            if panel.is_some_and(|p| IsDialogMessageW(p, &msg) != 0) {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        app.quitting.set(true);
        let shutdown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            app.backend.borrow_mut().shutdown()
        }))
        .unwrap_or_else(|_| Err("Desktop backend panicked during shutdown".into()));
        let panel = app.panel.borrow_mut().take();
        if let Some(p) = panel {
            if IsWindow(p.hwnd) != 0 {
                DestroyWindow(p.hwnd);
            }
        }
        let taskbar = app.taskbar.borrow_mut().take();
        drop(taskbar);
        app.remove_tray();
        if IsWindow(hwnd) != 0 {
            DestroyWindow(hwnd);
        }
        APP.with(|a| a.borrow_mut().take());
        drop(app);
        UnregisterClassW(name.as_ptr(), instance);
        if !old_dpi.is_null() {
            set_thread_dpi(old_dpi);
        }
        failure.map_or(shutdown, Err)
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(app) = current() {
            app.message(hwnd, msg, w, l)
        } else {
            DefWindowProcW(hwnd, msg, w, l)
        }
    }))
    .unwrap_or_else(|_| {
        PostQuitMessage(1);
        0
    })
}
impl App {
    unsafe fn populate_history(&self, list: HWND, rows: &[HistoryRow]) {
        SendMessageW(list, WM_SETREDRAW, 0, 0);
        SendMessageW(list, LVM_DELETEALLITEMS, 0, 0);
        for (index, row) in rows.iter().enumerate() {
            for (sub, value) in [
                &row.date,
                &row.upload,
                &row.download,
                &row.mixed,
                &row.total,
            ]
            .into_iter()
            .enumerate()
            {
                let mut text = wide(value);
                let item = LVITEMW {
                    mask: LVIF_TEXT,
                    iItem: index as i32,
                    iSubItem: sub as i32,
                    pszText: text.as_mut_ptr(),
                    ..zeroed()
                };
                SendMessageW(
                    list,
                    if sub == 0 {
                        LVM_INSERTITEMW
                    } else {
                        LVM_SETITEMTEXTW
                    },
                    index,
                    &item as *const _ as isize,
                );
            }
        }
        SendMessageW(list, WM_SETREDRAW, 1, 0);
        InvalidateRect(list, null(), 1);
    }
    unsafe fn choose_field(&self, id: usize) {
        let field = self.panel.borrow().as_ref().and_then(|p| {
            p.fields
                .iter()
                .find(|(_, h, _)| GetDlgCtrlID(*h) == id as i32)
                .cloned()
        });
        let Some((key, hwnd, kind)) = field else {
            return;
        };
        let value = window_text(hwnd);
        if kind == SettingKind::List {
            self.open_list_editor(hwnd, &value);
            return;
        }
        if kind == SettingKind::Font || key.ends_with("font_name") {
            let mut font: LOGFONTW = zeroed();
            font.lfHeight = -14;
            for (to, from) in font
                .lfFaceName
                .iter_mut()
                .take(31)
                .zip(value.encode_utf16())
            {
                *to = from
            }
            let mut chooser: CHOOSEFONTW = zeroed();
            chooser.lStructSize = size_of::<CHOOSEFONTW>() as u32;
            chooser.hwndOwner = GetAncestor(hwnd, GA_ROOT);
            chooser.lpLogFont = &mut font;
            chooser.Flags = CF_SCREENFONTS | CF_INITTOLOGFONTSTRUCT | CF_EFFECTS;
            if ChooseFontW(&mut chooser) != 0 {
                let end = font.lfFaceName.iter().position(|v| *v == 0).unwrap_or(32);
                SetWindowTextW(
                    hwnd,
                    wide(&String::from_utf16_lossy(&font.lfFaceName[..end])).as_ptr(),
                );
                let size = (chooser.iPointSize / 10).max(5).to_string();
                let style = u8::from(font.lfWeight >= 700)
                    | (u8::from(font.lfItalic != 0) * 2)
                    | (u8::from(font.lfUnderline != 0) * 4)
                    | (u8::from(font.lfStrikeOut != 0) * 8);
                let prefix = key.strip_suffix("font_name").unwrap_or("");
                let fields = self
                    .panel
                    .borrow()
                    .as_ref()
                    .map(|p| p.fields.clone())
                    .unwrap_or_default();
                for (field, h, _) in fields {
                    if field == format!("{prefix}font_size") {
                        SetWindowTextW(h, wide(&size).as_ptr());
                    }
                    if field == format!("{prefix}font_style") {
                        SetWindowTextW(h, wide(&style.to_string()).as_ptr());
                    }
                    if let Some(bit) = field
                        .strip_prefix(&format!("{prefix}font_bit_"))
                        .and_then(|s| s.parse::<u8>().ok())
                    {
                        SendMessageW(h, BM_SETCHECK, usize::from(style & bit != 0), 0);
                    }
                }
            }
            return;
        }
        let mut colors = [0u32; 16];
        let mut chooser: CHOOSECOLORW = zeroed();
        chooser.lStructSize = size_of::<CHOOSECOLORW>() as u32;
        chooser.hwndOwner = GetAncestor(hwnd, GA_ROOT);
        chooser.lpCustColors = colors.as_mut_ptr();
        chooser.rgbResult = if value.starts_with('#') {
            parse_color(&value).unwrap_or(0)
        } else {
            value.parse().unwrap_or(0)
        };
        chooser.Flags = CC_RGBINIT | CC_FULLOPEN;
        if ChooseColorW(&mut chooser) != 0 {
            let color = chooser.rgbResult;
            SetWindowTextW(
                hwnd,
                wide(&if kind == SettingKind::Color {
                    color.to_string()
                } else {
                    format_color(color)
                })
                .as_ptr(),
            );
        }
    }
    unsafe fn open_list_editor(&self, target: HWND, value: &str) {
        self.close_list_editor(false);
        let parent = GetAncestor(target, GA_ROOT);
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            wide(CLASS).as_ptr(),
            wide(&self.label("edit_list", "Edit selected items")).as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            580,
            480,
            parent,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        let list = self.control(
            hwnd,
            "LISTBOX",
            "",
            WS_TABSTOP | WS_VSCROLL | LBS_EXTENDEDSEL as u32,
            [20, 20, 520, 285],
            0,
        );
        for v in parse_list_csv(value) {
            SendMessageW(list, LB_ADDSTRING, 0, wide(&v).as_ptr() as isize);
        }
        let entry = self.control(
            hwnd,
            "COMBOBOX",
            "",
            WS_TABSTOP | CBS_DROPDOWN as u32,
            [20, 320, 330, 180],
            0,
        );
        let d = self.dashboard.borrow().clone();
        for item in d.items {
            SendMessageW(entry, CB_ADDSTRING, 0, wide(&item.id).as_ptr() as isize);
        }
        self.control(
            hwnd,
            "BUTTON",
            "Add item",
            WS_TABSTOP,
            [370, 320, 170, 28],
            250,
        );
        self.control(
            hwnd,
            "BUTTON",
            "Remove selected",
            WS_TABSTOP,
            [20, 380, 180, 30],
            251,
        );
        self.control(
            hwnd,
            "BUTTON",
            "OK",
            WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            [310, 380, 110, 30],
            252,
        );
        self.control(
            hwnd,
            "BUTTON",
            "Cancel",
            WS_TABSTOP,
            [430, 380, 110, 30],
            253,
        );
        *self.list_editor.borrow_mut() = Some(ListEditor {
            hwnd,
            target,
            parent,
            list,
            entry,
        });
        EnableWindow(parent, 0);
        ShowWindow(hwnd, SW_SHOW);
        SetFocus(entry);
    }
    unsafe fn list_edit_add(&self) {
        let snapshot = self
            .list_editor
            .borrow()
            .as_ref()
            .map(|e| (e.list, e.entry));
        if let Some((list, entry)) = snapshot {
            let value = window_text(entry);
            let value = value.trim();
            if !value.is_empty() {
                SendMessageW(list, LB_ADDSTRING, 0, wide(value).as_ptr() as isize);
                SetWindowTextW(entry, wide("").as_ptr());
            }
        }
    }
    unsafe fn list_edit_remove(&self) {
        let list = self.list_editor.borrow().as_ref().map(|e| e.list);
        if let Some(list) = list {
            let count = SendMessageW(list, LB_GETCOUNT, 0, 0);
            for i in (0..count).rev() {
                if SendMessageW(list, LB_GETSEL, i as usize, 0) > 0 {
                    SendMessageW(list, LB_DELETESTRING, i as usize, 0);
                }
            }
        }
    }
    unsafe fn close_list_editor(&self, save: bool) {
        let editor = self.list_editor.borrow_mut().take();
        if let Some(e) = editor {
            if save {
                let mut values = vec![];
                let count = SendMessageW(e.list, LB_GETCOUNT, 0, 0);
                for i in 0..count {
                    let len = SendMessageW(e.list, LB_GETTEXTLEN, i as usize, 0).max(0) as usize;
                    let mut text = vec![0u16; len + 1];
                    SendMessageW(e.list, LB_GETTEXT, i as usize, text.as_mut_ptr() as isize);
                    values.push(String::from_utf16_lossy(&text[..len]));
                }
                SetWindowTextW(e.target, wide(&encode_list_csv(&values)).as_ptr());
            }
            EnableWindow(e.parent, 1);
            DestroyWindow(e.hwnd);
            SetForegroundWindow(e.parent);
        }
    }
    unsafe fn plugin_at(&self, hwnd: HWND, x: i32, y: i32) -> Option<String> {
        if self.panel.borrow().as_ref().is_some_and(|p| p.hwnd == hwnd) {
            return None;
        }
        let taskbar = hwnd != self.main.get();
        if taskbar {
            let rects = self.taskbar_item_rects.borrow();
            if !rects.is_empty() {
                let hit = rects
                    .iter()
                    .find(|(_, r)| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
                    .map(|(id, _)| id);
                let dashboard = self.dashboard.borrow();
                return hit
                    .filter(|id| {
                        dashboard
                            .taskbar_items
                            .iter()
                            .chain(&dashboard.items)
                            .any(|i| &i.id == *id && i.plugin)
                    })
                    .cloned();
            }
        }
        let o = self.options.borrow().clone();
        let d = self.dashboard.borrow().clone();
        let source = ordered_items(&o, &d, taskbar);
        let ids: Vec<_> = source.iter().map(|i| i.id.clone()).collect();
        let mut bounds = zeroed();
        GetClientRect(hwnd, &mut bounds);
        let rects = item_rects(&o, &ids, bounds.right, bounds.bottom, taskbar);
        ids.into_iter()
            .zip(rects)
            .find(|(id, r)| {
                x >= r[0]
                    && x < r[2]
                    && y >= r[1]
                    && y < r[3]
                    && source.iter().any(|i| i.id == *id && i.plugin)
            })
            .map(|(id, _)| id)
    }
    unsafe fn plugin_mouse(&self, hwnd: HWND, kind: i32, l: LPARAM, screen: bool) -> bool {
        let mut point = POINT {
            x: (l as u16 as i16) as i32,
            y: ((l >> 16) as u16 as i16) as i32,
        };
        if screen {
            ScreenToClient(hwnd, &mut point);
        }
        let Some(id) = self.plugin_at(hwnd, point.x, point.y) else {
            return false;
        };
        *self.active_plugin.borrow_mut() = Some(id.clone());
        let event = PluginMouseEvent {
            kind,
            x: point.x,
            y: point.y,
            window: hwnd as usize,
            taskbar: hwnd != self.main.get(),
        };
        self.backend
            .try_borrow_mut()
            .is_ok_and(|mut b| b.plugin_mouse(&id, event))
    }
    fn queue(&self, action: DesktopAction) {
        self.actions.borrow_mut().push_back(action);
        unsafe {
            PostMessageW(self.main.get(), REFRESH, 0, 0);
        }
    }
    fn label(&self, key: &str, fallback: &str) -> String {
        self.backend
            .try_borrow()
            .map(|b| {
                b.text(
                    crate::localization::legacy_key(key),
                    crate::localization::fallback(key, fallback, &b.locale()),
                )
            })
            .unwrap_or_else(|_| fallback.into())
    }
    unsafe fn error(&self, text: &str) {
        MessageBoxW(
            self.main.get(),
            wide(text).as_ptr(),
            wide("TrafficMonitor").as_ptr(),
            MB_OK | MB_ICONWARNING,
        );
    }
    unsafe fn refresh(&self) {
        if self.quitting.get() {
            return;
        }
        let Ok(mut backend) = self.backend.try_borrow_mut() else {
            return;
        };
        let actions: Vec<_> = self.actions.borrow_mut().drain(..).collect();
        let refresh_panel = !actions.is_empty();
        let translate_panel = actions
            .iter()
            .any(|a| matches!(a, DesktopAction::SelectLanguage(_))
                || matches!(a,DesktopAction::SaveSettings(values) if values.iter().any(|(key,_)| key == "general/language")));
        let mut errors = vec![];
        let mut preference_result = None;
        for a in actions {
            let preferences = matches!(&a, DesktopAction::ApplyPreferences { .. });
            match backend.dispatch(a) {
                Ok(()) => {
                    if preferences {
                        preference_result = Some(true);
                    }
                }
                Err(e) => {
                    if preferences {
                        preference_result = Some(false);
                    }
                    errors.push(e);
                }
            }
        }
        match backend.sample() {
            Ok(d) => *self.dashboard.borrow_mut() = d,
            Err(e) => self.dashboard.borrow_mut().status = e,
        }
        let options = backend.options();
        drop(backend);
        if *self.options.borrow() != options {
            if let Err(e) = validate_options(&options) {
                errors.push(e)
            } else {
                *self.options.borrow_mut() = options;
                self.apply_window_options();
            }
        }
        if let Some(success) = preference_result {
            self.finish_preferences_apply(success);
        }
        self.ensure_taskbar();
        self.update_tray();
        self.update_main_tooltip();
        let o = self.options.borrow().clone();
        let recovery = o.show_taskbar && self.taskbar.borrow().is_none() && !self.tray.get();
        let visible =
            (o.show_main || recovery) && !(o.hide_fullscreen && self.foreground_fullscreen());
        ShowWindow(
            self.main.get(),
            if visible { SW_SHOWNOACTIVATE } else { SW_HIDE },
        );
        InvalidateRect(self.main.get(), null(), 0);
        let taskbar = self.taskbar.borrow().as_ref().map(|t| t.hwnd);
        if let Some(h) = taskbar {
            InvalidateRect(h, null(), 0);
        }
        if refresh_panel {
            self.refresh_panel_data();
        }
        if translate_panel {
            let page = self.panel.borrow().as_ref().map(|p| p.page);
            if let Some(page) = page {
                self.open_panel(page);
            }
        }
        let notices = std::mem::take(&mut self.dashboard.borrow_mut().notices);
        for notice in notices {
            self.notify(&notice)
        }
        for e in errors {
            self.error(&e)
        }
    }
    unsafe fn apply_window_options(&self) {
        self.apply_window_options_at(None);
    }
    unsafe fn apply_window_options_at(&self, suggested: Option<RECT>) {
        self.taskbar_desired_width.set(0);
        self.taskbar_item_rects.borrow_mut().clear();
        let o = self.options.borrow().clone();
        let hwnd = self.main.get();
        let icon_path = o
            .tray_icon
            .clone()
            .unwrap_or_else(|| o.resource_dir.join("TrafficMonitor.ico"));
        if *self.icon_path.borrow() != icon_path && icon_path.is_file() {
            let icon = LoadImageW(
                null_mut(),
                wide(&icon_path.to_string_lossy()).as_ptr(),
                IMAGE_ICON,
                0,
                0,
                LR_LOADFROMFILE | LR_DEFAULTSIZE,
            ) as HICON;
            if !icon.is_null() {
                let old = self.icon.replace(icon);
                *self.icon_path.borrow_mut() = icon_path;
                SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, icon as isize);
                SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icon as isize);
                let panel = self.panel.borrow().as_ref().map(|p| p.hwnd);
                if let Some(panel) = panel {
                    SendMessageW(panel, WM_SETICON, ICON_SMALL as usize, icon as isize);
                }
                if !old.is_null() {
                    DestroyIcon(old);
                }
            }
        }
        let dpi = window_dpi(hwnd).max(96) as i32;
        let font = CreateFontW(
            font_height(o.font_size, dpi as u32),
            0,
            0,
            0,
            if o.font_style & 1 != 0 { 700 } else { 400 },
            u32::from(o.font_style & 2 != 0),
            u32::from(o.font_style & 4 != 0),
            u32::from(o.font_style & 8 != 0),
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            wide(&o.font_name).as_ptr(),
        );
        let old = self.font.replace(font);
        if !old.is_null() {
            DeleteObject(old);
        }
        let style = o.taskbar_style.font_style;
        let taskbar_font = CreateFontW(
            font_height(o.taskbar_style.font_size, dpi as u32),
            0,
            0,
            0,
            if style & 1 != 0 { 700 } else { 400 },
            u32::from(style & 2 != 0),
            u32::from(style & 4 != 0),
            u32::from(style & 8 != 0),
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            wide(&o.taskbar_style.font_name).as_ptr(),
        );
        let old = self.taskbar_font.replace(taskbar_font);
        if !old.is_null() {
            DeleteObject(old);
        }
        let system_dark = system_dark_theme();
        self.theme_background.set(if o.follow_system_theme {
            if system_dark { 0x00202020 } else { 0x00f0f0f0 }
        } else {
            o.background
        });
        self.theme_foreground.set(if o.follow_system_theme {
            if system_dark { 0x00f0f0f0 } else { 0x00202020 }
        } else {
            o.foreground
        });
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            (WS_EX_TOOLWINDOW
                | WS_EX_LAYERED
                | if o.click_through {
                    WS_EX_TRANSPARENT
                } else {
                    0
                }) as _,
        );
        SetLayeredWindowAttributes(hwnd, 0, o.opacity, LWA_ALPHA);
        let target = monitor_bounds(
            &o,
            dpi as u32,
            suggested.map(|r| [r.left, r.top, r.right, r.bottom]),
        );
        SetWindowPos(
            hwnd,
            if o.topmost {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            },
            target[0],
            target[1],
            target[2] - target[0],
            target[3] - target[1],
            SWP_NOACTIVATE,
        );
        SetTimer(hwnd, 1, o.interval_ms, None);
        let path = o.skin.as_ref().and_then(|s| s.background.clone());
        if *self.skin_path.borrow() != path {
            let old = self.skin.replace(null_mut());
            if !old.is_null() {
                GdipDisposeImage(old.cast());
            }
            if let Some(p) = &path {
                let mut image = null_mut();
                if GdipCreateBitmapFromFile(wide(&p.to_string_lossy()).as_ptr(), &mut image) == 0 {
                    self.skin.set(image);
                }
            }
            *self.skin_path.borrow_mut() = path;
        }
        if uses_png(&o) && !self.skin.get().is_null() {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (style & !WS_EX_LAYERED) as _);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (style | WS_EX_LAYERED) as _);
        }
        self.apply_skin_region();
        if !o.show_tray {
            self.remove_tray();
        } else {
            self.update_tray();
        }
        let previous = self.taskbar.borrow_mut().take();
        drop(previous);
        self.ensure_taskbar();
        let previous = self.main_tooltip.borrow_mut().take();
        drop(previous);
        if o.tooltip {
            self.create_main_tooltip();
        }
    }
    unsafe fn create_main_tooltip(&self) {
        let target = self.main.get();
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST,
            windows_sys::core::w!("tooltips_class32"),
            null(),
            WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            target,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        let mut tooltip = Tooltip {
            hwnd,
            text: wide("TrafficMonitor"),
            target,
        };
        let mut info: TTTOOLINFOW = zeroed();
        info.cbSize = size_of::<TTTOOLINFOW>() as u32;
        info.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
        info.hwnd = target;
        info.uId = target as usize;
        info.lpszText = tooltip.text.as_mut_ptr();
        *self.main_tooltip.borrow_mut() = Some(tooltip);
        SendMessageW(hwnd, TTM_ADDTOOLW, 0, &info as *const _ as isize);
        SendMessageW(hwnd, TTM_SETMAXTIPWIDTH, 0, 600);
    }
    unsafe fn apply_skin_region(&self) {
        let o = self.options.borrow().clone();
        let path = o.skin.as_ref().and_then(|s| s.mask.as_ref());
        let Some(path) = path else {
            SetWindowRgn(self.main.get(), null_mut(), 1);
            return;
        };
        let mut bitmap = null_mut();
        if GdipCreateBitmapFromFile(wide(&path.to_string_lossy()).as_ptr(), &mut bitmap) != 0 {
            SetWindowRgn(self.main.get(), null_mut(), 1);
            return;
        }
        let mut width = 0;
        let mut height = 0;
        GdipGetImageWidth(bitmap.cast(), &mut width);
        GdipGetImageHeight(bitmap.cast(), &mut height);
        let mut bounds = zeroed();
        GetClientRect(self.main.get(), &mut bounds);
        let region = CreateRectRgn(0, 0, 0, 0);
        let mut included = false;
        if width > 0 && height > 0 {
            for y in 0..bounds.bottom {
                let mut start = None;
                for x in 0..=bounds.right {
                    let mut color = 0;
                    let visible = if x < bounds.right {
                        GdipBitmapGetPixel(
                            bitmap,
                            (i64::from(x) * i64::from(width) / i64::from(bounds.right.max(1)))
                                as i32,
                            (i64::from(y) * i64::from(height) / i64::from(bounds.bottom.max(1)))
                                as i32,
                            &mut color,
                        );
                        mask_opaque(color)
                    } else {
                        false
                    };
                    if visible {
                        if start.is_none() {
                            start = Some(x);
                        }
                    } else if let Some(left) = start.take() {
                        let row = CreateRectRgn(left, y, x, y + 1);
                        CombineRgn(region, region, row, RGN_OR);
                        DeleteObject(row);
                        included = true;
                    }
                }
            }
        }
        GdipDisposeImage(bitmap.cast());
        if !included {
            SetRectRgn(region, 0, 0, bounds.right, bounds.bottom);
        }
        if SetWindowRgn(self.main.get(), region, 1) == 0 {
            DeleteObject(region);
        }
    }
    unsafe fn update_main_tooltip(&self) {
        let text = self.dashboard.borrow().tooltip.clone();
        let snapshot = {
            let mut tooltip = self.main_tooltip.borrow_mut();
            tooltip.as_mut().map(|t| {
                t.text = wide(&text);
                (t.hwnd, t.target, t.text.as_mut_ptr())
            })
        };
        if let Some((hwnd, target, text)) = snapshot {
            let mut info: TTTOOLINFOW = zeroed();
            info.cbSize = size_of::<TTTOOLINFOW>() as u32;
            info.uFlags = TTF_IDISHWND;
            info.hwnd = target;
            info.uId = target as usize;
            info.lpszText = text;
            SendMessageW(hwnd, TTM_UPDATETIPTEXTW, 0, &info as *const _ as isize);
        }
    }
    unsafe fn foreground_fullscreen(&self) -> bool {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() || hwnd == self.main.get() {
            return false;
        }
        let mut class = [0u16; 80];
        GetClassNameW(hwnd, class.as_mut_ptr(), 80);
        let class = String::from_utf16_lossy(&class);
        if class.starts_with("Progman")
            || class.starts_with("WorkerW")
            || class.starts_with("Shell_")
        {
            return false;
        }
        let mut r = zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 {
            return false;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..zeroed()
        };
        GetMonitorInfoW(monitor, &mut info);
        r.left <= info.rcMonitor.left
            && r.top <= info.rcMonitor.top
            && r.right >= info.rcMonitor.right
            && r.bottom >= info.rcMonitor.bottom
    }
    unsafe fn tray_data(&self) -> NOTIFYICONDATAW {
        let mut n: NOTIFYICONDATAW = zeroed();
        n.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        n.hWnd = self.main.get();
        n.uID = 1;
        n.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        n.uCallbackMessage = TRAY;
        n.hIcon = self.icon.get();
        let d = self.dashboard.borrow();
        let text = if d.tooltip.is_empty() {
            "TrafficMonitor"
        } else {
            &d.tooltip
        };
        let mut tip = [0u16; 128];
        for (to, from) in tip.iter_mut().take(127).zip(text.encode_utf16()) {
            *to = from
        }
        n.szTip = tip;
        n
    }
    unsafe fn update_tray(&self) {
        if !self.options.borrow().show_tray {
            return;
        }
        let n = self.tray_data();
        let operation = if self.tray.get() { NIM_MODIFY } else { NIM_ADD };
        if Shell_NotifyIconW(operation, &n) != 0 {
            self.tray.set(true);
        }
    }
    unsafe fn remove_tray(&self) {
        if self.tray.replace(false) {
            let n = self.tray_data();
            Shell_NotifyIconW(NIM_DELETE, &n);
        }
    }
    unsafe fn notify(&self, text: &str) {
        let mut n = self.tray_data();
        n.uFlags = NIF_INFO;
        let mut info = [0u16; 256];
        for (to, from) in info.iter_mut().take(255).zip(text.encode_utf16()) {
            *to = from
        }
        n.szInfo = info;
        let mut title = [0u16; 64];
        for (to, from) in title.iter_mut().zip("TrafficMonitor".encode_utf16()) {
            *to = from
        }
        n.szInfoTitle = title;
        Shell_NotifyIconW(NIM_MODIFY, &n);
    }
    unsafe fn ensure_taskbar(&self) {
        let o = self.options.borrow().clone();
        let has_items = !ordered_items(&o, &self.dashboard.borrow(), true).is_empty();
        let old_invalid = self.taskbar.borrow().as_ref().is_some_and(|t| {
            let mut r: RECT = zeroed();
            GetClientRect(t.parent, &mut r);
            let buttons_changed = t.displaced.is_some_and(|(hwnd, _, expected)| {
                if IsWindow(hwnd) == 0 {
                    return true;
                }
                let mut actual = zeroed();
                GetWindowRect(hwnd, &mut actual);
                MapWindowPoints(
                    null_mut(),
                    GetParent(hwnd),
                    &mut actual as *mut RECT as *mut POINT,
                    2,
                );
                actual.left != expected.left
                    || actual.top != expected.top
                    || actual.right != expected.right
                    || actual.bottom != expected.bottom
            });
            let mut child: RECT = zeroed();
            GetClientRect(t.hwnd, &mut child);
            let width_changed = r.right >= r.bottom
                && self.taskbar_desired_width.get() > 0
                && child.right != self.taskbar_desired_width.get();
            IsWindow(t.hwnd) == 0
                || IsWindow(t.parent) == 0
                || t.parent_size != (r.right, r.bottom)
                || buttons_changed
                || width_changed
        });
        if old_invalid || !o.show_taskbar || !has_items {
            let old = self.taskbar.borrow_mut().take();
            drop(old);
        }
        if !o.show_taskbar || !has_items {
            self.taskbar_desired_width.set(0);
            self.taskbar_item_rects.borrow_mut().clear();
            return;
        }
        if self.taskbar.borrow().is_none() {
            if let Ok(hosts) = probe_desktop_taskbars() {
                if let Some(host) = hosts.get(o.taskbar_monitor).or_else(|| hosts.first()) {
                    let parent = host.taskbar.window.value() as HWND;
                    let mut r = zeroed();
                    GetClientRect(parent, &mut r);
                    let scale = window_dpi(parent).max(96) as i32;
                    let vertical = r.bottom > r.right;
                    let width = (if o.taskbar_style.auto_width {
                        self.taskbar_desired_width.get()
                    } else {
                        (o.taskbar_width * scale / 96).max(self.taskbar_desired_width.get())
                    })
                    .min(if vertical { r.bottom } else { r.right })
                    .max(1);
                    let mut x = (r.right - width - o.taskbar_offset * scale / 96).max(0);
                    let mut y = 0;
                    let mut height = r.bottom;
                    if let Some(tray) = &host.notification_area {
                        x = (tray.screen_rect.left
                            - host.taskbar.client_screen_rect.left
                            - width
                            - o.taskbar_offset * scale / 96)
                            .max(0);
                    } else if !vertical {
                        x = (r.right
                            - width
                            - o.taskbar_style.right_space * scale / 96
                            - o.taskbar_offset * scale / 96)
                            .max(0);
                    }
                    if vertical {
                        x = 0;
                        y = (r.bottom - width - o.taskbar_offset).max(0);
                        height = width;
                    }
                    if o.taskbar_style.place_left {
                        if vertical {
                            y = o.taskbar_offset.max(0);
                        } else {
                            x = host.start_button.as_ref().map_or(0, |s| {
                                s.screen_rect.right - host.taskbar.client_screen_rect.left
                            }) + o.taskbar_offset;
                        }
                    }
                    if !vertical && is_windows11() {
                        let widgets = o.taskbar_style.avoid_widgets
                            && registry_dword(
                                HKEY_CURRENT_USER,
                                windows_sys::core::w!(
                                    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced"
                                ),
                                windows_sys::core::w!("TaskbarDa"),
                                0,
                            ) != 0;
                        let centered = registry_dword(
                            HKEY_CURRENT_USER,
                            windows_sys::core::w!(
                                "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced"
                            ),
                            windows_sys::core::w!("TaskbarAl"),
                            1,
                        ) != 0;
                        if o.taskbar_style.place_left && centered {
                            let start = host
                                .start_button
                                .as_ref()
                                .map(|s| s.screen_rect.left - host.taskbar.client_screen_rect.left);
                            x = if o.taskbar_style.snap_to_icons {
                                start.map_or(0, |left| left - width - 2)
                            } else if widgets {
                                o.taskbar_style.left_space * scale / 96
                            } else {
                                2
                            };
                            x += o.taskbar_offset * scale / 96;
                        } else if o.taskbar_style.place_left {
                            x =
                                host.notification_area.as_ref().map_or(
                                    r.right - o.taskbar_style.right_space * scale / 96,
                                    |t| t.screen_rect.left - host.taskbar.client_screen_rect.left,
                                ) - width
                                    + o.taskbar_offset * scale / 96;
                        }
                        if widgets && !centered {
                            x -= o.taskbar_style.left_space * scale / 96;
                        }
                        x = x.clamp(0, (r.right - width).max(0));
                    }
                    y += o.taskbar_style.top_offset * scale / 96;
                    let font = CreateFontW(
                        font_height(o.taskbar_style.font_size, scale as u32),
                        0,
                        0,
                        0,
                        if o.taskbar_style.font_style & 1 != 0 {
                            700
                        } else {
                            400
                        },
                        u32::from(o.taskbar_style.font_style & 2 != 0),
                        u32::from(o.taskbar_style.font_style & 4 != 0),
                        u32::from(o.taskbar_style.font_style & 8 != 0),
                        DEFAULT_CHARSET as u32,
                        OUT_DEFAULT_PRECIS as u32,
                        CLIP_DEFAULT_PRECIS as u32,
                        CLEARTYPE_QUALITY as u32,
                        DEFAULT_PITCH as u32,
                        wide(&o.taskbar_style.font_name).as_ptr(),
                    );
                    let previous = self.taskbar_font.replace(font);
                    if !previous.is_null() {
                        DeleteObject(previous);
                    }
                    // Explorer composes its XAML taskbar through a redirected surface.
                    // Use a layered child even for an opaque monitor to remain visible.
                    let mut layered = true;
                    let dpi_scope = WindowDpiScope::enter(parent);
                    let mut child = CreateWindowExW(
                        WS_EX_NOACTIVATE | if layered { WS_EX_LAYERED } else { 0 },
                        wide(CLASS).as_ptr(),
                        wide("TrafficMonitor taskbar").as_ptr(),
                        WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                        x,
                        y,
                        if vertical { r.right } else { width },
                        height,
                        parent,
                        null_mut(),
                        GetModuleHandleW(null()),
                        null(),
                    );
                    if child.is_null() && layered {
                        child = CreateWindowExW(
                            WS_EX_NOACTIVATE,
                            wide(CLASS).as_ptr(),
                            wide("TrafficMonitor taskbar").as_ptr(),
                            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                            x,
                            y,
                            if vertical { r.right } else { width },
                            height,
                            parent,
                            null_mut(),
                            GetModuleHandleW(null()),
                            null(),
                        );
                        layered = false;
                        self.notify("Taskbar transparency is unavailable. The executable must declare Windows 8/10 support in its application manifest; an opaque taskbar monitor is shown.");
                    }
                    drop(dpi_scope);
                    if !child.is_null() {
                        self.taskbar_generation
                            .set(self.taskbar_generation.get().saturating_add(1));
                        if layered {
                            SetLayeredWindowAttributes(
                                child,
                                o.taskbar_style.transparent_color.unwrap_or(0),
                                o.taskbar_style.opacity,
                                LWA_ALPHA
                                    | if o.taskbar_style.transparent_color.is_some() {
                                        LWA_COLORKEY
                                    } else {
                                        0
                                    },
                            );
                        }
                        let mut displaced = None;
                        if !vertical {
                            if let Some(buttons) = &host.task_buttons {
                                let bh = buttons.window.value() as HWND;
                                let mut before = zeroed();
                                GetWindowRect(bh, &mut before);
                                MapWindowPoints(
                                    null_mut(),
                                    GetParent(bh),
                                    &mut before as *mut RECT as *mut POINT,
                                    2,
                                );
                                let mut boundary = POINT { x, y: 0 };
                                MapWindowPoints(parent, GetParent(bh), &mut boundary, 1);
                                if (before.right > boundary.x && boundary.x > before.left)
                                    || o.taskbar_style.place_left
                                {
                                    let after = if o.taskbar_style.place_left {
                                        RECT {
                                            left: (boundary.x + width).max(before.left),
                                            ..before
                                        }
                                    } else {
                                        RECT {
                                            right: boundary.x,
                                            ..before
                                        }
                                    };
                                    if after.right > after.left
                                        && MoveWindow(
                                            bh,
                                            after.left,
                                            after.top,
                                            after.right - after.left,
                                            after.bottom - after.top,
                                            1,
                                        ) != 0
                                    {
                                        displaced = Some((bh, before, after));
                                    }
                                }
                            }
                        }
                        *self.taskbar.borrow_mut() = Some(Taskbar {
                            hwnd: child,
                            parent,
                            displaced,
                            parent_size: (r.right, r.bottom),
                            tooltip: null_mut(),
                            tooltip_text: wide("TrafficMonitor"),
                        });
                        if o.taskbar_style.tooltip {
                            self.create_taskbar_tooltip(child);
                        }
                        if let Ok(mut b) = self.backend.try_borrow_mut() {
                            b.attach_windows(self.main.get() as usize, Some(child as usize));
                        }
                    }
                }
            }
        }
        if let Some((h, p)) = self.taskbar.borrow().as_ref().map(|t| (t.hwnd, t.parent)) {
            ShowWindow(
                h,
                if IsWindowVisible(p) != 0 {
                    SW_SHOWNOACTIVATE
                } else {
                    SW_HIDE
                },
            );
            // Explorer's XAML bridge occupies the complete taskbar client area.
            // Keep our child above that sibling without activating either process.
            SetWindowPos(
                h,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        if let Ok(mut b) = self.backend.try_borrow_mut() {
            b.attach_windows(
                self.main.get() as usize,
                self.taskbar.borrow().as_ref().map(|t| t.hwnd as usize),
            );
        }
        self.update_taskbar_tooltip();
    }
    unsafe fn create_taskbar_tooltip(&self, child: HWND) {
        let tooltip = CreateWindowExW(
            WS_EX_TOPMOST,
            windows_sys::core::w!("tooltips_class32"),
            null(),
            WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            child,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if tooltip.is_null() {
            return;
        }
        let text = {
            let mut t = self.taskbar.borrow_mut();
            let Some(t) = t.as_mut() else {
                DestroyWindow(tooltip);
                return;
            };
            t.tooltip = tooltip;
            t.tooltip_text.as_mut_ptr()
        };
        let mut info: TTTOOLINFOW = zeroed();
        info.cbSize = size_of::<TTTOOLINFOW>() as u32;
        info.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
        info.hwnd = child;
        info.uId = child as usize;
        info.lpszText = text;
        SendMessageW(tooltip, TTM_ADDTOOLW, 0, &info as *const _ as isize);
        SendMessageW(tooltip, TTM_SETMAXTIPWIDTH, 0, 600);
    }
    unsafe fn update_taskbar_tooltip(&self) {
        let text = self.dashboard.borrow().tooltip.clone();
        let snapshot = {
            let mut t = self.taskbar.borrow_mut();
            t.as_mut().filter(|t| !t.tooltip.is_null()).map(|t| {
                t.tooltip_text = wide(&text);
                (t.tooltip, t.hwnd, t.tooltip_text.as_mut_ptr())
            })
        };
        if let Some((tooltip, child, text)) = snapshot {
            let mut info: TTTOOLINFOW = zeroed();
            info.cbSize = size_of::<TTTOOLINFOW>() as u32;
            info.uFlags = TTF_IDISHWND;
            info.hwnd = child;
            info.uId = child as usize;
            info.lpszText = text;
            SendMessageW(tooltip, TTM_UPDATETIPTEXTW, 0, &info as *const _ as isize);
        }
    }
    unsafe fn measure_taskbar_layout(
        &self,
        dc: HDC,
        items: &[DisplayValue],
        o: &DesktopOptions,
        width: i32,
        height: i32,
    ) -> Vec<[i32; 4]> {
        let widths: Vec<_> = items
            .iter()
            .map(|item| {
                let requested = if item.custom_draw {
                    self.backend
                        .try_borrow_mut()
                        .ok()
                        .and_then(|mut b| b.plugin_width(&item.id, dc as usize, true))
                        .or(item.width)
                } else {
                    None
                };
                let measured = requested.unwrap_or_else(|| {
                    let digits = if matches!(item.id.as_str(), "up" | "down" | "total_speed") {
                        o.taskbar_style.value_digits
                    } else {
                        3
                    };
                    let sample = taskbar_measurement_value(&item.value, digits);
                    [&item.value, &sample]
                        .into_iter()
                        .map(|value| {
                            let text = wide(&if o.taskbar_style.show_labels {
                                format!("{} {}", item.label, value)
                            } else {
                                value.clone()
                            });
                            let mut size: SIZE = zeroed();
                            GetTextExtentPoint32W(
                                dc,
                                text.as_ptr(),
                                text.len().saturating_sub(1) as i32,
                                &mut size,
                            );
                            size.cx
                        })
                        .max()
                        .unwrap_or(0)
                });
                (measured.clamp(16, 4096), item.double_line)
            })
            .collect();
        let gap = o.item_gap.max(0);
        let (rects, needed) = taskbar_item_layout(&widths, height, o.taskbar_two_rows, gap);
        let parent = self.taskbar.borrow().as_ref().map(|t| t.parent);
        let mut host: RECT = zeroed();
        if let Some(parent) = parent {
            GetClientRect(parent, &mut host);
        }
        let dpi = parent.map_or(96, |h| window_dpi(h).max(96)) as i32;
        if host.bottom > host.right && host.right > 0 {
            return item_rects(
                o,
                &items.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
                width,
                height,
                false,
            );
        }
        let desired = taskbar_panel_width(
            needed,
            if o.taskbar_style.auto_width {
                None
            } else {
                Some(o.taskbar_width * dpi / 96)
            },
            host.right.max(width),
        );
        if self.taskbar_desired_width.replace(desired) != desired {
            PostMessageW(self.main.get(), REFRESH, 0, 0);
        }
        *self.taskbar_item_rects.borrow_mut() = items
            .iter()
            .zip(&rects)
            .map(|(i, r)| (i.id.clone(), *r))
            .collect();
        rects
    }
    unsafe fn paint(&self, hwnd: HWND) {
        let paint = PaintSession::begin(hwnd);
        let mut bounds = zeroed();
        GetClientRect(hwnd, &mut bounds);
        let mut o = self.options.borrow().clone();
        let taskbar_mode = hwnd != self.main.get();
        let text_mode = TextMode {
            font: if taskbar_mode {
                o.taskbar_style.font_name.clone()
            } else {
                o.font_name.clone()
            },
            points: if taskbar_mode {
                o.taskbar_style.font_size
            } else {
                o.font_size
            },
            style: if taskbar_mode {
                o.taskbar_style.font_style
            } else {
                o.font_style
            },
            dpi: window_dpi(hwnd),
            directwrite: !o.taskbar_style.disable_d2d,
            colorful: o.taskbar_style.colorful_emoji,
        };
        let surface = if hwnd == self.main.get() && uses_png(&o) && !self.skin.get().is_null() {
            DibSurface::new(paint.dc, bounds.right, bounds.bottom)
        } else {
            None
        };
        let dc = surface.as_ref().map_or(paint.dc, |s| s.dc);
        let _state = DcState::save(dc);
        let d = self.dashboard.borrow().clone();
        let taskbar = hwnd != self.main.get();
        let panel = self
            .panel
            .borrow()
            .as_ref()
            .filter(|p| p.hwnd == hwnd)
            .map(|p| (p.page, p.aux.clone()))
            .or_else(|| {
                self.list_editor
                    .borrow()
                    .as_ref()
                    .filter(|e| e.hwnd == hwnd)
                    .map(|_| (Page::Advanced, vec![]))
            });
        if taskbar && panel.is_none() {
            o.foreground = o.taskbar_style.foreground;
            o.background = o.taskbar_style.background;
            o.visible_items = o.taskbar_style.visible_items.clone();
            if o.taskbar_style.auto_background {
                let mut r: RECT = zeroed();
                GetWindowRect(hwnd, &mut r);
                let parent = GetParent(hwnd);
                let mut p: RECT = zeroed();
                GetWindowRect(parent, &mut p);
                let x = if r.left > p.left { r.left - 1 } else { r.right };
                if x >= p.left && x < p.right {
                    let screen = GetDC(null_mut());
                    if !screen.is_null() {
                        let color = GetPixel(screen, x, (r.top + r.bottom) / 2);
                        ReleaseDC(null_mut(), screen);
                        if color != CLR_INVALID && color != 0 {
                            o.background = color;
                        }
                    }
                }
            }
        } else if panel.is_none() {
            o.foreground = self.theme_foreground.get();
            o.background = self.theme_background.get();
        }
        let color = if panel.is_some() {
            0x00f6f6f6
        } else {
            o.background
        };
        let brush = CreateSolidBrush(color);
        if surface.is_none() {
            FillRect(dc, &bounds, brush);
        }
        DeleteObject(brush);
        let old = SelectObject(
            dc,
            if panel.is_some() {
                self.panel_font.get()
            } else if taskbar {
                self.taskbar_font.get()
            } else {
                self.font.get()
            },
        );
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(
            dc,
            if panel.is_some() {
                0x00202020
            } else {
                o.foreground
            },
        );
        if let Some((page, aux)) = panel {
            if page == Page::History {
                let log = aux
                    .get(3)
                    .is_some_and(|h| SendMessageW(*h, BM_GETCHECK, 0, 0) == BST_CHECKED as isize);
                self.paint_history(
                    dc,
                    &d.history,
                    history_chart_rect(
                        window_dpi(hwnd),
                        GetScrollPos(hwnd, SB_HORZ),
                        GetScrollPos(hwnd, SB_VERT),
                    ),
                    log,
                );
            }
            SelectObject(dc, old);
            return;
        }
        if !taskbar && !self.skin.get().is_null() {
            if let Some(surface) = &surface {
                surface.draw_background(self.skin.get());
            } else {
                let mut graphics = null_mut();
                if GdipCreateFromHDC(dc, &mut graphics) == 0 {
                    GdipDrawImageRectI(
                        graphics,
                        self.skin.get().cast(),
                        0,
                        0,
                        bounds.right,
                        bounds.bottom,
                    );
                    GdipDeleteGraphics(graphics);
                }
            }
        }
        let background = surface
            .as_ref()
            .map(|s| s.pixels().to_vec())
            .unwrap_or_default();
        let items = ordered_items(&o, &d, taskbar);
        let ids: Vec<_> = items.iter().map(|i| i.id.clone()).collect();
        let rects = if taskbar {
            self.measure_taskbar_layout(dc, &items, &o, bounds.right, bounds.bottom)
        } else {
            item_rects(&o, &ids, bounds.right, bounds.bottom, false)
        };
        for (item, mut coords) in items.iter().zip(rects) {
            if taskbar {
                let margin = o
                    .taskbar_style
                    .vertical_margin
                    .max(0)
                    .min((coords[3] - coords[1]) / 3);
                coords[1] += margin;
                coords[3] -= margin;
            }
            if item.custom_draw {
                let _plugin_state = DcState::save(dc);
                if let Ok(mut b) = self.backend.try_borrow_mut() {
                    if b.draw_plugin(&item.id, dc as usize, coords, o.dark, taskbar) {
                        continue;
                    }
                }
            }
            let skin_item = if taskbar {
                None
            } else {
                o.skin
                    .as_ref()
                    .and_then(|s| s.items.iter().find(|s| s.id == item.id))
            };
            let color = skin_item
                .and_then(|s| s.color)
                .or(item.color)
                .unwrap_or(o.foreground);
            SetTextColor(dc, color);
            let graph = if taskbar {
                graph_policy(item, &o.taskbar_style)
            } else {
                None
            };
            if let Some((false, maximum)) = graph {
                let f = item
                    .history
                    .last()
                    .map(|value| value / maximum)
                    .or(item.fraction)
                    .unwrap_or(0.0);
                let f = if f.is_finite() { f } else { 0.0 };
                let bar = RECT {
                    left: coords[0],
                    top: coords[3] - 3,
                    right: coords[0] + ((coords[2] - coords[0]) as f64 * f.clamp(0.0, 1.0)) as i32,
                    bottom: coords[3],
                };
                let graph_color = o.taskbar_style.graph_color.unwrap_or(color);
                let brush = CreateSolidBrush(graph_color);
                FillRect(dc, &bar, brush);
                DeleteObject(brush);
                if o.taskbar_style.show_graph_dashed_box {
                    let pen = CreatePen(PS_DOT, 1, graph_color);
                    let previous = SelectObject(dc, pen);
                    let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                    Rectangle(dc, coords[0], coords[1], coords[2], coords[3]);
                    SelectObject(dc, old_brush);
                    SelectObject(dc, previous);
                    DeleteObject(pen);
                }
            }
            if let Some((true, max)) = graph.filter(|_| item.history.len() > 1) {
                let pen = CreatePen(
                    PS_SOLID,
                    1,
                    if taskbar {
                        o.taskbar_style.graph_color.unwrap_or(color)
                    } else {
                        color
                    },
                );
                let previous = SelectObject(dc, pen);
                for (i, v) in item.history.iter().enumerate() {
                    let x = coords[0]
                        + i as i32 * (coords[2] - coords[0]) / (item.history.len() - 1) as i32;
                    let y = coords[3] - chart_height(*v, max, (coords[3] - coords[1]) / 3, false);
                    if taskbar && o.taskbar_style.graph_style == GraphStyle::Bar {
                        MoveToEx(dc, x, coords[3], null_mut());
                        LineTo(dc, x, y);
                    } else if i == 0 {
                        MoveToEx(dc, x, y, null_mut());
                    } else {
                        LineTo(dc, x, y);
                    }
                }
                SelectObject(dc, previous);
                DeleteObject(pen);
            }
            let text = if skin_item.is_some_and(|s| !s.show_label)
                || (taskbar && !o.taskbar_style.show_labels)
            {
                item.value.clone()
            } else {
                format!("{} {}", item.label, item.value)
            };
            let mut r = RECT {
                left: coords[0],
                top: coords[1],
                right: coords[2],
                bottom: coords[3],
            };
            let item_colors = if taskbar {
                o.taskbar_style.item_colors.iter().find(|c| c.id == item.id)
            } else {
                None
            };
            if skin_item.is_some_and(|s| s.alignment == TextAlignment::Side)
                || (taskbar
                    && o.taskbar_style.show_labels
                    && (o.taskbar_style.label_color.is_some()
                        || o.taskbar_style.value_color.is_some()
                        || item_colors.is_some()))
            {
                SetTextColor(
                    dc,
                    if taskbar {
                        item_colors
                            .and_then(|c| c.label)
                            .or(o.taskbar_style.label_color)
                            .unwrap_or(color)
                    } else {
                        color
                    },
                );
                draw_text(
                    dc,
                    surface.as_ref(),
                    &item.label,
                    &mut r,
                    DT_SINGLELINE | DT_VCENTER | DT_LEFT,
                    &text_mode,
                );
                SetTextColor(
                    dc,
                    if taskbar {
                        item_colors
                            .and_then(|c| c.value)
                            .or(o.taskbar_style.value_color)
                            .unwrap_or(color)
                    } else {
                        color
                    },
                );
                draw_text(
                    dc,
                    surface.as_ref(),
                    &item.value,
                    &mut r,
                    DT_SINGLELINE | DT_VCENTER | DT_RIGHT,
                    &text_mode,
                );
                continue;
            }
            draw_text(
                dc,
                surface.as_ref(),
                &text,
                &mut r,
                DT_SINGLELINE
                    | DT_VCENTER
                    | DT_END_ELLIPSIS
                    | if skin_item.is_some_and(|s| s.alignment == TextAlignment::Center) {
                        DT_CENTER
                    } else if skin_item
                        .is_some_and(|s| s.align_right || s.alignment == TextAlignment::Right)
                        || (taskbar && o.taskbar_style.align_right)
                    {
                        DT_RIGHT
                    } else {
                        DT_LEFT
                    },
                &text_mode,
            );
        }
        if items.is_empty() {
            let text = if d.status.is_empty() {
                "Waiting for first sample…"
            } else {
                &d.status
            };
            draw_text(
                dc,
                surface.as_ref(),
                text,
                &mut bounds,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                &text_mode,
            );
        }
        SelectObject(dc, old);
        if let Some(surface) = &surface {
            surface.present(hwnd, o.opacity, &background);
        }
    }
    unsafe fn paint_history(&self, dc: HDC, rows: &[HistoryRow], r: [i32; 4], log: bool) {
        let max = rows.iter().map(|r| r.total_bytes).max().unwrap_or(1) as f64;
        let width = (r[2] - r[0]).max(1);
        let count = rows.len().max(1);
        let brush = CreateSolidBrush(0x00c08030);
        for (i, row) in rows.iter().enumerate() {
            let height = chart_height(row.total_bytes as f64, max, r[3] - r[1] - 20, log);
            let bar = RECT {
                left: r[0] + i as i32 * width / count as i32,
                top: r[3] - height,
                right: r[0] + (i + 1) as i32 * width / count as i32 - 1,
                bottom: r[3],
            };
            FillRect(dc, &bar, brush);
        }
        DeleteObject(brush);
    }
    unsafe fn menu(&self, hwnd: HWND) {
        let menu = CreatePopupMenu();
        let o = self.options.borrow().clone();
        let entries = [
            ("settings", "Settings…", 100, false),
            ("history", "History…", 101, false),
            ("network", "Network connections…", 102, false),
            ("appearance", "Skins and language…", 103, false),
            ("plugins", "Plugins…", 104, false),
            ("taskbar_settings", "Taskbar appearance…", 114, false),
            ("show_main", "Show floating monitor", 105, o.show_main),
            ("show_taskbar", "Show taskbar monitor", 106, o.show_taskbar),
            ("topmost", "Always on top", 107, o.topmost),
            ("lock", "Lock position", 108, o.lock_position),
            ("click_through", "Mouse click through", 109, o.click_through),
            ("update", "Check for updates", 110, false),
            ("data", "Open data folder", 111, false),
            ("exit", "Exit", 112, false),
        ];
        for (key, text, id, checked) in entries {
            AppendMenuW(
                menu,
                MF_STRING | if checked { MF_CHECKED } else { 0 },
                id,
                wide(&self.label(key, text)).as_ptr(),
            );
        }
        let mut point = zeroed();
        GetCursorPos(&mut point);
        SetForegroundWindow(self.main.get());
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            self.main.get(),
            null(),
        );
        DestroyMenu(menu);
        PostMessageW(hwnd, WM_NULL, 0, 0);
        if command != 0 {
            self.command(command as usize);
        }
    }
    unsafe fn command(&self, id: usize) {
        match id {
            1 | 2 => {
                if self.list_editor.borrow().is_some() {
                    self.close_list_editor(id == IDOK as usize);
                } else if id == IDOK as usize {
                    self.command(223);
                } else {
                    self.close_panel();
                }
            }
            100 => self.open_panel(Page::Settings),
            101 => self.open_panel(Page::History),
            102 => self.open_panel(Page::Network),
            103 => self.open_panel(Page::Appearance),
            104 => self.open_panel(Page::Plugins),
            114 => self.open_panel(Page::Taskbar),
            105..=109 => {
                let mut o = self.options.borrow().clone();
                match id {
                    105 => o.show_main = !o.show_main,
                    106 => o.show_taskbar = !o.show_taskbar,
                    107 => o.topmost = !o.topmost,
                    108 => o.lock_position = !o.lock_position,
                    _ => o.click_through = !o.click_through,
                }
                self.queue(DesktopAction::ApplyOptions(o));
            }
            110 => self.queue(DesktopAction::CheckUpdates),
            111 => self.queue(DesktopAction::OpenDataDirectory),
            112 => {
                PostMessageW(self.main.get(), WM_CLOSE, 0, 0);
            }
            200 => {
                self.apply_panel();
            }
            223 => {
                if self.apply_panel() {
                    let pending = self
                        .panel
                        .borrow()
                        .as_ref()
                        .and_then(|p| p.settings.as_ref())
                        .is_some_and(|s| s.pending.is_some());
                    if pending {
                        if let Some(s) = self
                            .panel
                            .borrow_mut()
                            .as_mut()
                            .and_then(|p| p.settings.as_mut())
                        {
                            s.close_after_apply = true;
                        }
                    } else {
                        self.close_panel();
                    }
                }
            }
            230..=233 => self.move_display_item(id),
            234 => self.recommend_taskbar_items(),
            201 => self.close_panel(),
            202 => self.open_panel(Page::Advanced),
            203 => self.open_panel(Page::Settings),
            204 => self.history_range(),
            205 => self.history_file(false),
            206 => self.history_file(true),
            207 => {
                let h = self.panel.borrow().as_ref().map(|p| p.hwnd);
                let checkbox = self
                    .panel
                    .borrow()
                    .as_ref()
                    .and_then(|p| p.aux.get(3).copied());
                if let Some(checkbox) = checkbox {
                    let mut o = self.options.borrow().clone();
                    o.history_log_scale =
                        SendMessageW(checkbox, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
                    self.queue(DesktopAction::ApplyOptions(o));
                }
                if let Some(h) = h {
                    InvalidateRect(h, null(), 1);
                }
            }
            208 => self.network_select(),
            209 => self.plugin_configure(),
            210 => self.plugin_toggle(),
            211 => self.plugin_command(),
            212 => self.apply_appearance(),
            213 => self.selection_details(),
            250 => self.list_edit_add(),
            251 => self.list_edit_remove(),
            252 => self.close_list_editor(true),
            253 => self.close_list_editor(false),
            3000..=6000 => self.choose_field(id - 3000),
            _ => {}
        }
    }
    unsafe fn close_panel(&self) {
        self.close_list_editor(false);
        let p = self.panel.borrow_mut().take();
        if let Some(p) = p {
            DestroyWindow(p.hwnd);
        }
    }
    unsafe fn message(&self, hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if msg == self.taskbar_created {
            self.tray.set(false);
            let old = self.taskbar.borrow_mut().take();
            drop(old);
            self.update_tray();
            self.ensure_taskbar();
            return 0;
        }
        match msg {
            WM_PAINT => {
                self.paint(hwnd);
                0
            }
            WM_ERASEBKGND => 1,
            WM_TIMER if hwnd == self.main.get() => {
                if w == 2 {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                } else {
                    self.refresh();
                }
                0
            }
            REFRESH => {
                self.refresh();
                0
            }
            WM_SIZE
                if self
                    .panel
                    .borrow()
                    .as_ref()
                    .is_some_and(|p| p.hwnd == hwnd && p.settings.is_some()) =>
            {
                self.resize_settings_panel();
                0
            }
            WM_COMMAND => {
                let id = w & 0xffff;
                if id == 225 {
                    self.update_fixed_width_control();
                }
                self.command(id);
                0
            }
            WM_VSCROLL | WM_HSCROLL => {
                let bar = if msg == WM_VSCROLL { SB_VERT } else { SB_HORZ };
                let mut info = SCROLLINFO {
                    cbSize: size_of::<SCROLLINFO>() as u32,
                    fMask: SIF_ALL,
                    ..zeroed()
                };
                GetScrollInfo(hwnd, bar, &mut info);
                let pos = match (w & 0xffff) as i32 {
                    SB_LINEUP => info.nPos - 30,
                    SB_LINEDOWN => info.nPos + 30,
                    SB_PAGEUP => info.nPos - info.nPage as i32,
                    SB_PAGEDOWN => info.nPos + info.nPage as i32,
                    SB_THUMBTRACK => info.nTrackPos,
                    SB_TOP => 0,
                    SB_BOTTOM => info.nMax,
                    _ => info.nPos,
                }
                .clamp(0, (info.nMax - info.nPage as i32 + 1).max(0));
                SetScrollPos(hwnd, bar, pos, 1);
                ScrollWindowEx(
                    hwnd,
                    if bar == SB_HORZ { info.nPos - pos } else { 0 },
                    if bar == SB_VERT { info.nPos - pos } else { 0 },
                    null(),
                    null(),
                    null_mut(),
                    null_mut(),
                    SW_SCROLLCHILDREN | SW_INVALIDATE | SW_ERASE,
                );
                0
            }
            WM_CONTEXTMENU => {
                self.menu(hwnd);
                0
            }
            WM_RBUTTONUP => {
                if !self.plugin_mouse(hwnd, 1, l, false) {
                    self.menu(hwnd);
                }
                0
            }
            WM_LBUTTONDOWN => {
                if self
                    .plugin_at(
                        hwnd,
                        (l as u16 as i16) as i32,
                        ((l >> 16) as u16 as i16) as i32,
                    )
                    .is_some()
                {
                    SetFocus(hwnd);
                }
                DefWindowProcW(hwnd, msg, w, l)
            }
            WM_LBUTTONUP => {
                if self.plugin_mouse(hwnd, 0, l, false) {
                    0
                } else {
                    DefWindowProcW(hwnd, msg, w, l)
                }
            }
            WM_MOUSEWHEEL => {
                if self.plugin_mouse(
                    hwnd,
                    if ((w >> 16) as u16 as i16) > 0 { 3 } else { 4 },
                    l,
                    true,
                ) {
                    0
                } else {
                    DefWindowProcW(hwnd, msg, w, l)
                }
            }
            WM_NCMOUSEMOVE if hwnd == self.main.get() => {
                let tooltip = self.main_tooltip.borrow().as_ref().map(|t| t.hwnd);
                if let Some(tooltip) = tooltip {
                    let mut msg: MSG = zeroed();
                    msg.hwnd = hwnd;
                    msg.message = WM_MOUSEMOVE;
                    msg.pt = POINT {
                        x: (l as u16 as i16) as i32,
                        y: ((l >> 16) as u16 as i16) as i32,
                    };
                    let mut point = msg.pt;
                    ScreenToClient(hwnd, &mut point);
                    msg.lParam =
                        ((point.y as u32 as usize) << 16 | point.x as u16 as usize) as isize;
                    SendMessageW(tooltip, TTM_RELAYEVENT, 0, &msg as *const _ as isize);
                }
                DefWindowProcW(hwnd, msg, w, l)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                if self.panel.borrow().as_ref().is_some_and(|p| p.hwnd == hwnd) {
                    return DefWindowProcW(hwnd, msg, w, l);
                }
                let id = self.active_plugin.borrow().clone();
                if let Some(id) = id {
                    let event = PluginKeyboardEvent {
                        key: w as i32,
                        ctrl: GetKeyState(VK_CONTROL as i32) < 0,
                        shift: GetKeyState(VK_SHIFT as i32) < 0,
                        alt: GetKeyState(VK_MENU as i32) < 0,
                        window: hwnd as usize,
                        taskbar: hwnd != self.main.get(),
                    };
                    if self
                        .backend
                        .try_borrow_mut()
                        .is_ok_and(|mut b| b.plugin_keyboard(&id, event))
                    {
                        return 0;
                    }
                }
                DefWindowProcW(hwnd, msg, w, l)
            }
            TRAY => {
                match l as u32 {
                    WM_RBUTTONUP | WM_CONTEXTMENU => self.menu(self.main.get()),
                    WM_LBUTTONDBLCLK => {
                        let mut o = self.options.borrow().clone();
                        o.show_main = true;
                        o.click_through = false;
                        self.queue(DesktopAction::ApplyOptions(o));
                        self.open_panel(Page::Settings)
                    }
                    _ => {}
                }
                0
            }
            WM_LBUTTONDBLCLK | WM_NCLBUTTONDBLCLK => {
                if self.plugin_mouse(hwnd, 2, l, msg == WM_NCLBUTTONDBLCLK) {
                    return 0;
                }
                let action = {
                    let o = self.options.borrow();
                    if hwnd == self.main.get() {
                        o.double_click
                    } else {
                        o.taskbar_double_click
                    }
                };
                match action {
                    DoubleClickAction::Settings => self.open_panel(Page::Settings),
                    DoubleClickAction::History => self.open_panel(Page::History),
                    DoubleClickAction::Network => self.open_panel(Page::Network),
                    DoubleClickAction::TaskManager => self.queue(DesktopAction::OpenTaskManager),
                    DoubleClickAction::ToggleMain => self.command(105),
                    DoubleClickAction::ToggleMore => self.queue(DesktopAction::ToggleMore),
                    DoubleClickAction::SpecificApp => self.queue(DesktopAction::OpenSpecificApp {
                        taskbar: hwnd != self.main.get(),
                    }),
                    DoubleClickAction::ChangeSkin => self.queue(DesktopAction::CycleSkin),
                    DoubleClickAction::None => {}
                }
                0
            }
            WM_NCHITTEST if hwnd == self.main.get() => {
                let o = self.options.borrow().clone();
                if o.click_through {
                    HTTRANSPARENT as isize
                } else if !o.lock_position {
                    let mut point = POINT {
                        x: (l as u16 as i16) as i32,
                        y: ((l >> 16) as u16 as i16) as i32,
                    };
                    ScreenToClient(hwnd, &mut point);
                    if self.plugin_at(hwnd, point.x, point.y).is_some() {
                        HTCLIENT as isize
                    } else {
                        HTCAPTION as isize
                    }
                } else {
                    HTCLIENT as isize
                }
            }
            WM_EXITSIZEMOVE if hwnd == self.main.get() => {
                let mut r = zeroed();
                GetWindowRect(hwnd, &mut r);
                let mut position = (r.left, r.top);
                if self.options.borrow().snap_to_edges {
                    let mut info = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..zeroed()
                    };
                    GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info);
                    position = snap(
                        [r.left, r.top, r.right, r.bottom],
                        [
                            info.rcWork.left,
                            info.rcWork.top,
                            info.rcWork.right,
                            info.rcWork.bottom,
                        ],
                        16,
                    );
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        position.0,
                        position.1,
                        0,
                        0,
                        SWP_NOZORDER | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
                self.queue(DesktopAction::PositionChanged {
                    x: position.0,
                    y: position.1,
                });
                0
            }
            WM_DPICHANGED if hwnd == self.main.get() && l != 0 => {
                let r = *(l as *const RECT);
                {
                    let mut options = self.options.borrow_mut();
                    options.x = r.left;
                    options.y = r.top;
                }
                self.queue(DesktopAction::PositionChanged {
                    x: r.left,
                    y: r.top,
                });
                self.apply_window_options_at(Some(r));
                0
            }
            WM_SETTINGCHANGE | WM_DISPLAYCHANGE | WM_THEMECHANGED => {
                if hwnd == self.main.get() {
                    let mut r = zeroed();
                    GetWindowRect(hwnd, &mut r);
                    self.apply_window_options_at(Some(r));
                }
                PostMessageW(self.main.get(), REFRESH, 0, 0);
                0
            }
            WM_NOTIFY if l != 0 => {
                let header = &*(l as *const NMHDR);
                if header.idFrom == 220 && header.code == TCN_SELCHANGE {
                    self.select_settings_tab(
                        SendMessageW(header.hwndFrom, TCM_GETCURSEL, 0, 0).max(0) as usize,
                    );
                }
                if header.idFrom == 214 && header.code == MCN_SELCHANGE {
                    let date = &*(l as *const NMSELCHANGE);
                    let value = format!(
                        "{:04}-{:02}-{:02}",
                        date.stSelStart.wYear, date.stSelStart.wMonth, date.stSelStart.wDay
                    );
                    let aux = self.panel.borrow().as_ref().map(|p| p.aux.clone());
                    if let Some(aux) = aux {
                        SetWindowTextW(aux[0], wide(&value).as_ptr());
                        SetWindowTextW(aux[1], wide(&value).as_ptr());
                        SendMessageW(aux[2], CB_SETCURSEL, 0, 0);
                        self.history_range();
                    }
                }
                0
            }
            WM_CLOSE => {
                if self
                    .list_editor
                    .borrow()
                    .as_ref()
                    .is_some_and(|e| e.hwnd == hwnd)
                {
                    self.close_list_editor(false);
                    return 0;
                }
                if hwnd == self.main.get() {
                    self.quitting.set(true);
                    KillTimer(hwnd, 1);
                    KillTimer(hwnd, 2);
                    PostQuitMessage(0);
                } else {
                    self.close_panel();
                }
                0
            }
            WM_QUERYENDSESSION if hwnd == self.main.get() => match self.backend.try_borrow_mut() {
                Ok(mut backend) => match backend.checkpoint() {
                    Ok(()) => 1,
                    Err(e) => {
                        eprintln!("Session checkpoint failed: {e}");
                        0
                    }
                },
                Err(_) => 0,
            },
            WM_ENDSESSION if hwnd == self.main.get() => {
                if w != 0 {
                    self.quitting.set(true);
                    KillTimer(hwnd, 1);
                    KillTimer(hwnd, 2);
                    PostQuitMessage(0);
                }
                0
            }
            _ => DefWindowProcW(hwnd, msg, w, l),
        }
    }
}
impl App {
    unsafe fn refresh_panel_data(&self) {
        let snapshot = self
            .panel
            .borrow()
            .as_ref()
            .map(|p| (p.page, p.hwnd, p.list, p.detail));
        let Some((page, hwnd, list, detail)) = snapshot else {
            return;
        };
        let d = self.dashboard.borrow().clone();
        match page {
            Page::History => {
                self.populate_history(list, &d.history);
                SetWindowTextW(detail, wide(&d.history_summary).as_ptr());
                InvalidateRect(hwnd, null(), 1);
            }
            Page::Plugins => {
                let index = SendMessageW(list, LB_GETCURSEL, 0, 0);
                SendMessageW(list, LB_RESETCONTENT, 0, 0);
                for p in &d.plugins {
                    SendMessageW(
                        list,
                        LB_ADDSTRING,
                        0,
                        wide(&format!(
                            "{} {} {}",
                            if p.enabled { "[enabled]" } else { "[disabled]" },
                            p.name,
                            p.version
                        ))
                        .as_ptr() as isize,
                    );
                }
                SendMessageW(list, LB_SETCURSEL, index.max(0) as usize, 0);
                self.selection_details();
            }
            _ => {}
        }
    }
    unsafe fn control(
        &self,
        parent: HWND,
        class: &str,
        text: &str,
        style: u32,
        r: [i32; 4],
        id: usize,
    ) -> HWND {
        let dpi = window_dpi(parent).max(96) as i32;
        let r = r.map(|v| v * dpi / 96);
        let label = if class == "BUTTON" || class == "STATIC" {
            self.label(text, text)
        } else {
            text.into()
        };
        let hwnd = CreateWindowExW(
            if class == "EDIT" || class == "LISTBOX" {
                WS_EX_CLIENTEDGE
            } else {
                0
            },
            wide(class).as_ptr(),
            wide(&label).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            r[0],
            r[1],
            r[2],
            r[3],
            parent,
            id as *mut c_void,
            GetModuleHandleW(null()),
            null(),
        );
        SendMessageW(hwnd, WM_SETFONT, self.panel_font.get() as usize, 1);
        hwnd
    }
    // Mirrors the control's label, bound key/value, type, geometry and Win32 ID.
    #[allow(clippy::too_many_arguments)]
    unsafe fn field(
        &self,
        parent: HWND,
        label: &str,
        key: &str,
        value: &str,
        kind: SettingKind,
        r: [i32; 4],
        id: usize,
    ) -> (String, HWND, SettingKind) {
        let hwnd = match &kind {
            SettingKind::Bool => {
                let h = self.control(
                    parent,
                    "BUTTON",
                    label,
                    WS_TABSTOP | BS_AUTOCHECKBOX as u32,
                    r,
                    id,
                );
                SendMessageW(
                    h,
                    BM_SETCHECK,
                    usize::from(value == "true" || value == "1"),
                    0,
                );
                h
            }
            SettingKind::Choice(choices) => {
                self.control(
                    parent,
                    "STATIC",
                    label,
                    SS_LEFTNOWORDWRAP | SS_ENDELLIPSIS,
                    [r[0], r[1], 340, 24],
                    0,
                );
                let h = self.control(
                    parent,
                    "COMBOBOX",
                    "",
                    WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
                    [r[0] + 350, r[1], r[2] - 350, 220],
                    id,
                );
                for (i, v) in choices.iter().enumerate() {
                    SendMessageW(
                        h,
                        CB_ADDSTRING,
                        0,
                        wide(&self.label(v, v)).as_ptr() as isize,
                    );
                    if v == value {
                        SendMessageW(h, CB_SETCURSEL, i, 0);
                    }
                }
                h
            }
            _ => {
                self.control(
                    parent,
                    "STATIC",
                    label,
                    SS_LEFTNOWORDWRAP | SS_ENDELLIPSIS,
                    [r[0], r[1] + 3, 340, 24],
                    0,
                );
                let picker = matches!(
                    kind,
                    SettingKind::Color | SettingKind::Font | SettingKind::List
                ) || key.ends_with("font_name")
                    || matches!(
                        key,
                        "foreground"
                            | "background"
                            | "tb_foreground"
                            | "tb_background"
                            | "tb_label_color"
                            | "tb_value_color"
                    );
                let h = self.control(
                    parent,
                    "EDIT",
                    value,
                    WS_TABSTOP | ES_AUTOHSCROLL as u32,
                    [
                        r[0] + 350,
                        r[1],
                        r[2] - 350 - if picker { 36 } else { 0 },
                        26,
                    ],
                    id,
                );
                if picker {
                    self.control(
                        parent,
                        "BUTTON",
                        "…",
                        WS_TABSTOP,
                        [r[0] + r[2] - 32, r[1], 32, 26],
                        3000 + id,
                    );
                }
                h
            }
        };
        (key.into(), hwnd, kind)
    }
    unsafe fn build_settings_tabs(&self, p: &mut Panel, o: &DesktopOptions, d: &Dashboard) {
        let tab = self.control(
            p.hwnd,
            "SysTabControl32",
            "",
            WS_TABSTOP | WS_CLIPSIBLINGS,
            [12, 12, 900, 610],
            220,
        );
        let mut pages = vec![];
        for (index, category) in SettingsTab::ALL.into_iter().enumerate() {
            let mut text = wide(&self.label(category.title(), category.title()));
            let item = TCITEMW {
                mask: TCIF_TEXT,
                pszText: text.as_mut_ptr(),
                ..zeroed()
            };
            SendMessageW(tab, TCM_INSERTITEMW, index, &item as *const _ as isize);
            let page = self.control(
                p.hwnd,
                "STATIC",
                "",
                WS_VSCROLL | WS_HSCROLL | WS_CLIPCHILDREN,
                [20, 50, 880, 550],
                0,
            );
            SetWindowLongPtrW(
                page,
                GWL_EXSTYLE,
                (GetWindowLongPtrW(page, GWL_EXSTYLE) as isize | WS_EX_CONTROLPARENT as isize) as _,
            );
            SetWindowSubclass(page, Some(settings_page_proc), 1, 0);
            pages.push((page, 20));
        }
        let mut entries: Vec<(SettingsTab, String, String, String, SettingKind)> = vec![];
        let mut add = |tab, key: &str, label: &str, value: String, kind| {
            entries.push((tab, key.into(), label.into(), value, kind));
        };
        for (tab, key, label, value) in [
            (
                SettingsTab::General,
                "show_main",
                "Show floating monitor",
                o.show_main,
            ),
            (
                SettingsTab::General,
                "show_taskbar",
                "Show taskbar monitor",
                o.show_taskbar,
            ),
            (
                SettingsTab::General,
                "show_tray",
                "Show notification icon",
                o.show_tray,
            ),
            (
                SettingsTab::General,
                "follow_system_theme",
                "Follow system theme",
                o.follow_system_theme,
            ),
            (SettingsTab::General, "dark", "Dark theme", o.dark),
            (SettingsTab::Floating, "topmost", "Always on top", o.topmost),
            (
                SettingsTab::Floating,
                "click_through",
                "Mouse click through",
                o.click_through,
            ),
            (
                SettingsTab::Floating,
                "lock_position",
                "Lock floating position",
                o.lock_position,
            ),
            (
                SettingsTab::Floating,
                "snap_to_edges",
                "Snap to screen edges",
                o.snap_to_edges,
            ),
            (
                SettingsTab::Floating,
                "hide_fullscreen",
                "Hide during fullscreen",
                o.hide_fullscreen,
            ),
            (SettingsTab::Floating, "tooltip", "Show tooltip", o.tooltip),
            (
                SettingsTab::Floating,
                "swap_up_down",
                "Swap upload / download",
                o.swap_up_down,
            ),
            (
                SettingsTab::Taskbar,
                "tb_auto_width",
                "Automatically fit content width",
                o.taskbar_style.auto_width,
            ),
            (
                SettingsTab::Taskbar,
                "taskbar_two_rows",
                "Taskbar two rows",
                o.taskbar_two_rows,
            ),
            (
                SettingsTab::Taskbar,
                "tb_show_labels",
                "Show labels",
                o.taskbar_style.show_labels,
            ),
            (
                SettingsTab::Taskbar,
                "tb_align_right",
                "Right align values",
                o.taskbar_style.align_right,
            ),
            (
                SettingsTab::Taskbar,
                "tb_show_graph",
                "Show history graphs",
                o.taskbar_style.show_graph,
            ),
            (
                SettingsTab::Taskbar,
                "tb_show_status_bar",
                "Show status bars",
                o.taskbar_style.show_status_bar,
            ),
            (
                SettingsTab::Taskbar,
                "tb_tooltip",
                "Show tooltip",
                o.taskbar_style.tooltip,
            ),
            (
                SettingsTab::Taskbar,
                "tb_place_left",
                "Place at left / top",
                o.taskbar_style.place_left,
            ),
            (
                SettingsTab::Taskbar,
                "tb_swap_up_down",
                "Swap upload / download",
                o.taskbar_style.swap_up_down,
            ),
        ] {
            add(tab, key, label, value.to_string(), SettingKind::Bool);
        }
        for (tab, key, label, value) in [
            (
                SettingsTab::General,
                "interval_ms",
                "Refresh interval (ms)",
                o.interval_ms.to_string(),
            ),
            (
                SettingsTab::Floating,
                "opacity",
                "Opacity (0–255)",
                o.opacity.to_string(),
            ),
            (
                SettingsTab::Floating,
                "font_name",
                "Font family",
                o.font_name.clone(),
            ),
            (
                SettingsTab::Floating,
                "font_size",
                "Font size",
                o.font_size.to_string(),
            ),
            (
                SettingsTab::Floating,
                "foreground",
                "Text color (#RRGGBB)",
                format_color(o.foreground),
            ),
            (
                SettingsTab::Floating,
                "background",
                "Background (#RRGGBB)",
                format_color(o.background),
            ),
            (
                SettingsTab::Floating,
                "width",
                "Window width",
                o.width.to_string(),
            ),
            (
                SettingsTab::Floating,
                "height",
                "Window height",
                o.height.to_string(),
            ),
            (
                SettingsTab::Taskbar,
                "taskbar_width",
                "Minimum taskbar width",
                o.taskbar_width.to_string(),
            ),
            (
                SettingsTab::Taskbar,
                "taskbar_offset",
                "Taskbar offset",
                o.taskbar_offset.to_string(),
            ),
            (
                SettingsTab::Taskbar,
                "taskbar_monitor",
                "Taskbar monitor (0 = main)",
                o.taskbar_monitor.to_string(),
            ),
            (
                SettingsTab::Taskbar,
                "item_gap",
                "Item spacing",
                o.item_gap.to_string(),
            ),
            (
                SettingsTab::Taskbar,
                "tb_font_name",
                "Font family",
                o.taskbar_style.font_name.clone(),
            ),
            (
                SettingsTab::Taskbar,
                "tb_font_size",
                "Font size",
                o.taskbar_style.font_size.to_string(),
            ),
            (
                SettingsTab::Taskbar,
                "tb_foreground",
                "Text color (#RRGGBB)",
                format_color(o.taskbar_style.foreground),
            ),
            (
                SettingsTab::Taskbar,
                "tb_background",
                "Background (#RRGGBB)",
                format_color(o.taskbar_style.background),
            ),
            (
                SettingsTab::Taskbar,
                "tb_label_color",
                "Label color (blank = text)",
                o.taskbar_style
                    .label_color
                    .map(format_color)
                    .unwrap_or_default(),
            ),
            (
                SettingsTab::Taskbar,
                "tb_value_color",
                "Value color (blank = text)",
                o.taskbar_style
                    .value_color
                    .map(format_color)
                    .unwrap_or_default(),
            ),
            (
                SettingsTab::Taskbar,
                "tb_opacity",
                "Opacity (0–255)",
                o.taskbar_style.opacity.to_string(),
            ),
            (
                SettingsTab::Taskbar,
                "tb_top_offset",
                "Vertical offset",
                o.taskbar_style.top_offset.to_string(),
            ),
            (
                SettingsTab::Taskbar,
                "tb_vertical_margin",
                "Vertical margin",
                o.taskbar_style.vertical_margin.to_string(),
            ),
            (
                SettingsTab::Taskbar,
                "tb_graph_maximum",
                "Graph max (blank = auto)",
                o.taskbar_style
                    .graph_maximum
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            ),
            (
                SettingsTab::Taskbar,
                "tb_graph_color",
                "Graph color (blank = text)",
                o.taskbar_style
                    .graph_color
                    .map(format_color)
                    .unwrap_or_default(),
            ),
            (
                SettingsTab::Taskbar,
                "tb_transparent_color",
                "Transparent color (optional)",
                o.taskbar_style
                    .transparent_color
                    .map(format_color)
                    .unwrap_or_default(),
            ),
        ] {
            add(tab, key, label, value, SettingKind::Text);
        }
        for (tab, prefix, style) in [
            (SettingsTab::Floating, "", o.font_style),
            (SettingsTab::Taskbar, "tb_", o.taskbar_style.font_style),
        ] {
            for (label, bit) in [
                ("Bold", 1),
                ("Italic", 2),
                ("Underline", 4),
                ("Strikeout", 8),
            ] {
                add(
                    tab,
                    &format!("{prefix}font_bit_{bit}"),
                    label,
                    (style & bit != 0).to_string(),
                    SettingKind::Bool,
                );
            }
        }
        add(
            SettingsTab::Floating,
            "double_click",
            "Double click action",
            double_click_name(o.double_click).into(),
            SettingKind::Choice(double_click_choices()),
        );
        add(
            SettingsTab::Taskbar,
            "tb_double_click",
            "Double click action",
            double_click_name(o.taskbar_double_click).into(),
            SettingKind::Choice(double_click_choices()),
        );
        add(
            SettingsTab::Taskbar,
            "tb_graph_style",
            "Graph style",
            if o.taskbar_style.graph_style == GraphStyle::Bar {
                "History bars"
            } else {
                "Current progress"
            }
            .into(),
            SettingKind::Choice(vec!["Current progress".into(), "History bars".into()]),
        );
        add(
            SettingsTab::General,
            "_auto_start",
            if d.auto_start_scheduler {
                "Auto start (Task Scheduler)"
            } else {
                "Auto start (Registry)"
            },
            d.auto_start_enabled.unwrap_or(false).to_string(),
            SettingKind::Bool,
        );
        for f in &d.settings {
            if !covered_by_native_editor(&f.key) {
                let label = if f.help.is_empty() {
                    f.label.clone()
                } else {
                    format!("{} ({})", f.label, f.help)
                };
                add(
                    category(&f.key),
                    &f.key,
                    &label,
                    f.value.clone(),
                    f.kind.clone(),
                );
            }
        }
        for (index, (category, key, label, value, kind)) in entries.into_iter().enumerate() {
            let i = SettingsTab::ALL
                .iter()
                .position(|c| *c == category)
                .unwrap();
            let (parent, y) = &mut pages[i];
            let id = if key == "tb_auto_width" {
                225
            } else {
                1000 + index
            };
            let translated = self.label(&key, &label);
            let field = self.field(
                *parent,
                &translated,
                &key,
                &value,
                kind,
                [16, *y, 820, 28],
                id,
            );
            if (o.skin.is_some() && matches!(key.as_str(), "width" | "height" | "background"))
                || (key == "_auto_start" && d.auto_start_enabled.is_none())
            {
                EnableWindow(field.1, 0);
                let picker = GetDlgItem(*parent, (3000 + id) as i32);
                if !picker.is_null() {
                    EnableWindow(picker, 0);
                }
            }
            p.fields.push(field);
            *y += 38;
        }
        let item_page = pages[3].0;
        self.control(
            item_page,
            "STATIC",
            "Floating window",
            0,
            [16, 16, 390, 25],
            0,
        );
        self.control(item_page, "STATIC", "Taskbar", 0, [444, 16, 390, 25], 0);
        let mut items = d.items.clone();
        items.extend(d.taskbar_items.clone());
        let catalog = item_catalog(&items);
        let main_items = self.make_item_picker(item_page, &catalog, &o.visible_items, true, 16, 0);
        let taskbar_items = self.make_item_picker(
            item_page,
            &catalog,
            &o.taskbar_style.visible_items,
            false,
            444,
            1,
        );
        if o.skin.is_some() {
            EnableWindow(main_items.list, 0);
        }
        for (id, x, title) in [
            (230, 16, "Move up"),
            (231, 146, "Move down"),
            (232, 444, "Move up"),
            (233, 574, "Move down"),
        ] {
            let button = self.control(
                item_page,
                "BUTTON",
                title,
                WS_TABSTOP,
                [x, 465, 120, 30],
                id,
            );
            if o.skin.is_some() && id < 232 {
                EnableWindow(button, 0);
            }
        }
        self.control(
            item_page,
            "BUTTON",
            "Recommended items",
            WS_TABSTOP,
            [704, 465, 130, 30],
            234,
        );
        self.control(
            item_page,
            "STATIC",
            if o.skin.is_some() {
                "Floating display items are defined by the selected skin."
            } else {
                "Check items to display; select a row and use Move up / Move down to change order."
            },
            0,
            [16, 510, 820, 50],
            0,
        );
        pages[3].1 = 580;
        let appearance_page = pages[6].0;
        // Schema skin adaptation options remain below the two dedicated selectors.
        let appearance_y = pages[6].1 + 8;
        self.control(
            appearance_page,
            "STATIC",
            "Skin",
            0,
            [16, appearance_y, 180, 28],
            0,
        );
        let skin = self.control(
            appearance_page,
            "COMBOBOX",
            "",
            WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
            [210, appearance_y, 625, 300],
            0,
        );
        self.control(
            appearance_page,
            "STATIC",
            "Language",
            0,
            [16, appearance_y + 45, 180, 28],
            0,
        );
        let language = self.control(
            appearance_page,
            "COMBOBOX",
            "",
            WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
            [210, appearance_y + 45, 625, 300],
            0,
        );
        for (h, choices, selected) in [
            (skin, &d.skins, &d.current_skin),
            (language, &d.languages, &d.current_language),
        ] {
            for (i, choice) in choices.iter().enumerate() {
                SendMessageW(h, CB_ADDSTRING, 0, wide(&choice.name).as_ptr() as isize);
                if &choice.id == selected {
                    SendMessageW(h, CB_SETCURSEL, i, 0);
                }
            }
        }
        pages[6].1 += 120;
        let mut baseline = read_fields(&p.fields);
        baseline.push((
            "visible_items".into(),
            picker_selected(&main_items).join(","),
        ));
        baseline.push((
            "tb_visible_items".into(),
            picker_selected(&taskbar_items).join(","),
        ));
        baseline.push(("config/skin_selected".into(), d.current_skin.clone()));
        baseline.push(("general/language".into(), d.current_language.clone()));
        p.settings = Some(SettingsPanel {
            tab,
            pages,
            baseline,
            pending: None,
            close_after_apply: false,
            main_items,
            taskbar_items,
            appearance: (skin, language),
        });
        for (text, id, x) in [("Apply", 200, 540), ("OK", 223, 660), ("Cancel", 201, 780)] {
            self.control(
                p.hwnd,
                "BUTTON",
                text,
                WS_TABSTOP
                    | if id == 223 {
                        BS_DEFPUSHBUTTON as u32
                    } else {
                        0
                    },
                [x, 640, 110, 32],
                id,
            );
        }
    }
    unsafe fn make_item_picker(
        &self,
        parent: HWND,
        catalog: &[(String, String)],
        selected: &[String],
        all_if_empty: bool,
        x: i32,
        id: usize,
    ) -> ItemPicker {
        let list = self.control(
            parent,
            "SysListView32",
            "",
            WS_TABSTOP | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_NOCOLUMNHEADER,
            [x, 48, 392, 402],
            226 + id,
        );
        SendMessageW(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            (LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize,
        );
        let dpi = window_dpi(parent) as i32;
        let column = LVCOLUMNW {
            mask: LVCF_WIDTH,
            cx: 360 * dpi / 96,
            ..zeroed()
        };
        SendMessageW(list, LVM_INSERTCOLUMNW, 0, &column as *const _ as isize);
        let mut entries: Vec<_> = selected
            .iter()
            .map(|id| {
                catalog
                    .iter()
                    .find(|(k, _)| k == id)
                    .cloned()
                    .unwrap_or_else(|| {
                        (
                            id.clone(),
                            format!("{} ({})", id, self.label("Unavailable", "Unavailable")),
                        )
                    })
            })
            .collect();
        for entry in catalog {
            if !entries.iter().any(|(id, _)| id == &entry.0) {
                entries.push(entry.clone());
            }
        }
        let picker = ItemPicker { list, entries };
        let selected = if selected.is_empty() && all_if_empty {
            picker
                .entries
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        } else {
            selected.to_vec()
        };
        self.populate_item_picker(&picker, &selected, 0);
        picker
    }
    unsafe fn populate_item_picker(
        &self,
        picker: &ItemPicker,
        selected: &[String],
        current: usize,
    ) {
        SendMessageW(picker.list, LVM_DELETEALLITEMS, 0, 0);
        for (index, (id, label)) in picker.entries.iter().enumerate() {
            let mut text = wide(&self.label(label, label));
            let item = LVITEMW {
                mask: LVIF_TEXT | LVIF_STATE,
                iItem: index as i32,
                pszText: text.as_mut_ptr(),
                stateMask: LVIS_STATEIMAGEMASK | LVIS_SELECTED,
                state: if selected.contains(id) {
                    2 << 12
                } else {
                    1 << 12
                } | if index == current { LVIS_SELECTED } else { 0 },
                ..zeroed()
            };
            SendMessageW(picker.list, LVM_INSERTITEMW, 0, &item as *const _ as isize);
            // LVS_EX_CHECKBOXES initializes inserted rows unchecked; set the saved state after insertion.
            SendMessageW(
                picker.list,
                LVM_SETITEMSTATE,
                index,
                &item as *const _ as isize,
            );
        }
    }
    unsafe fn recommend_taskbar_items(&self) {
        let mut panel = self.panel.borrow_mut();
        let Some(s) = panel.as_mut().and_then(|p| p.settings.as_mut()) else {
            return;
        };
        let recommended: Vec<String> = ["up", "down", "cpu", "memory"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut entries: Vec<_> = recommended
            .iter()
            .filter_map(|id| {
                s.taskbar_items
                    .entries
                    .iter()
                    .find(|(key, _)| key == id)
                    .cloned()
            })
            .collect();
        entries.extend(
            s.taskbar_items
                .entries
                .iter()
                .filter(|(id, _)| !recommended.contains(id))
                .cloned(),
        );
        s.taskbar_items.entries = entries;
        self.populate_item_picker(&s.taskbar_items, &recommended, 0);
    }
    unsafe fn move_display_item(&self, id: usize) {
        let mut panel = self.panel.borrow_mut();
        let Some(s) = panel.as_mut().and_then(|p| p.settings.as_mut()) else {
            return;
        };
        let picker = if id < 232 {
            &mut s.main_items
        } else {
            &mut s.taskbar_items
        };
        let index = SendMessageW(
            picker.list,
            LVM_GETNEXTITEM,
            usize::MAX,
            LVNI_SELECTED as isize,
        );
        if index < 0 {
            return;
        }
        let next = if id % 2 == 0 {
            (index as usize).saturating_sub(1)
        } else {
            (index as usize + 1).min(picker.entries.len().saturating_sub(1))
        };
        if next == index as usize {
            return;
        }
        let selected = picker_selected(picker);
        picker.entries.swap(index as usize, next);
        self.populate_item_picker(picker, &selected, next);
        SendMessageW(picker.list, LVM_ENSUREVISIBLE, next, 0);
    }
    unsafe fn select_settings_tab(&self, index: usize) {
        let snapshot = self
            .panel
            .borrow()
            .as_ref()
            .and_then(|p| p.settings.as_ref().map(|s| (s.tab, s.pages.clone())));
        if let Some((tab, pages)) = snapshot {
            SendMessageW(tab, TCM_SETCURSEL, index, 0);
            for (i, (page, _)) in pages.iter().enumerate() {
                ShowWindow(*page, if i == index { SW_SHOW } else { SW_HIDE });
            }
        }
    }
    unsafe fn update_fixed_width_control(&self) {
        let fields = self
            .panel
            .borrow()
            .as_ref()
            .map(|p| p.fields.clone())
            .unwrap_or_default();
        let automatic = fields
            .iter()
            .find(|(k, _, _)| k == "tb_auto_width")
            .is_none_or(|(_, h, _)| SendMessageW(*h, BM_GETCHECK, 0, 0) == BST_CHECKED as isize);
        if let Some((_, h, _)) = fields.iter().find(|(k, _, _)| k == "taskbar_width") {
            SendMessageW(*h, EM_SETREADONLY, usize::from(automatic), 0);
            EnableWindow(*h, i32::from(!automatic));
        }
    }
    unsafe fn resize_settings_panel(&self) {
        let snapshot = self.panel.borrow().as_ref().and_then(|p| {
            p.settings
                .as_ref()
                .map(|s| (p.hwnd, s.tab, s.pages.clone()))
        });
        let Some((hwnd, tab, pages)) = snapshot else {
            return;
        };
        let mut r: RECT = zeroed();
        GetClientRect(hwnd, &mut r);
        let dpi = window_dpi(hwnd).max(96) as i32;
        let unit = |v: i32| v * dpi / 96;
        MoveWindow(
            tab,
            unit(12),
            unit(12),
            (r.right - unit(24)).max(1),
            (r.bottom - unit(74)).max(1),
            1,
        );
        for (page, content_height) in pages {
            let mut old: SCROLLINFO = zeroed();
            old.cbSize = size_of::<SCROLLINFO>() as u32;
            old.fMask = SIF_POS;
            GetScrollInfo(page, SB_VERT, &mut old);
            let old_y = old.nPos;
            GetScrollInfo(page, SB_HORZ, &mut old);
            let old_x = old.nPos;
            MoveWindow(
                page,
                unit(20),
                unit(50),
                (r.right - unit(40)).max(1),
                (r.bottom - unit(120)).max(1),
                1,
            );
            let mut client: RECT = zeroed();
            GetClientRect(page, &mut client);
            for (bar, max, size, old_pos) in [
                (SB_VERT, unit(content_height), client.bottom, old_y),
                (SB_HORZ, unit(860), client.right, old_x),
            ] {
                let pos = old_pos.min((max - size).max(0));
                let info = SCROLLINFO {
                    cbSize: size_of::<SCROLLINFO>() as u32,
                    fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                    nMin: 0,
                    nMax: max - 1,
                    nPage: size.max(0) as u32,
                    nPos: pos,
                    nTrackPos: 0,
                };
                SetScrollInfo(page, bar, &info, 1);
                if pos != old_pos {
                    ScrollWindowEx(
                        page,
                        if bar == SB_HORZ { old_pos - pos } else { 0 },
                        if bar == SB_VERT { old_pos - pos } else { 0 },
                        null(),
                        null(),
                        null_mut(),
                        null_mut(),
                        SW_SCROLLCHILDREN | SW_INVALIDATE,
                    );
                }
            }
        }
        for (i, id) in [200, 223, 201].into_iter().enumerate() {
            MoveWindow(
                GetDlgItem(hwnd, id),
                (r.right - unit(380 - i as i32 * 120)).max(unit(8)),
                (r.bottom - unit(48)).max(0),
                unit(110),
                unit(32),
                1,
            );
        }
    }
    unsafe fn open_panel(&self, page: Page) {
        let settings_tab = match page {
            Page::Settings => Some(0),
            Page::Taskbar => Some(2),
            Page::Advanced => Some(5),
            Page::Appearance => Some(6),
            _ => None,
        };
        if let Some(index) = settings_tab {
            let existing = self
                .panel
                .borrow()
                .as_ref()
                .filter(|p| p.settings.is_some())
                .map(|p| p.hwnd);
            if let Some(hwnd) = existing {
                self.select_settings_tab(index);
                ShowWindow(hwnd, SW_SHOW);
                SetForegroundWindow(hwnd);
                return;
            }
        }
        self.close_panel();
        let titles = if settings_tab.is_some() {
            "Settings"
        } else {
            match page {
                Page::Settings => "Settings",
                Page::Advanced => "Advanced settings",
                Page::Taskbar => "Taskbar appearance",
                Page::History => "Traffic history",
                Page::Network => "Network connections",
                Page::Appearance => "Skins and language",
                Page::Plugins => "Plugin manager",
            }
        };
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            wide(CLASS).as_ptr(),
            wide(&format!("TrafficMonitor — {}", self.label(titles, titles))).as_ptr(),
            WS_OVERLAPPED
                | WS_CAPTION
                | WS_SYSMENU
                | WS_MINIMIZEBOX
                | if settings_tab.is_some() {
                    WS_THICKFRAME
                } else {
                    WS_VSCROLL | WS_HSCROLL
                },
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            940,
            740,
            self.main.get(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if hwnd.is_null() {
            self.error("Could not create management window");
            return;
        }
        let dpi = window_dpi(hwnd).max(96) as i32;
        SendMessageW(
            hwnd,
            WM_SETICON,
            ICON_SMALL as usize,
            self.icon.get() as isize,
        );
        let font = CreateFontW(
            font_height(10, dpi as u32),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            windows_sys::core::w!("Segoe UI"),
        );
        let old = self.panel_font.replace(font);
        if !old.is_null() {
            DeleteObject(old);
        }
        let mut monitor = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..zeroed()
        };
        GetMonitorInfoW(
            MonitorFromWindow(self.main.get(), MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        );
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            (940 * dpi / 96).min(monitor.rcWork.right - monitor.rcWork.left),
            (740 * dpi / 96).min(monitor.rcWork.bottom - monitor.rcWork.top),
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        let o = self.options.borrow().clone();
        let d = self.dashboard.borrow().clone();
        let mut p = Panel {
            hwnd,
            page,
            fields: vec![],
            list: null_mut(),
            detail: null_mut(),
            aux: vec![],
            settings: None,
        };
        if settings_tab.is_some() {
            p.page = Page::Settings;
            self.build_settings_tabs(&mut p, &o, &d);
        } else {
            match page {
                Page::Settings | Page::Taskbar | Page::Advanced | Page::Appearance => {
                    unreachable!()
                }
                Page::History => {
                    self.control(hwnd, "STATIC", "From (YYYY-MM-DD)", 0, [20, 15, 160, 24], 0);
                    let from = self.control(
                        hwnd,
                        "EDIT",
                        &d.history_from,
                        WS_TABSTOP | ES_AUTOHSCROLL as u32,
                        [180, 12, 130, 28],
                        0,
                    );
                    self.control(hwnd, "STATIC", "To", 0, [330, 15, 30, 24], 0);
                    let to = self.control(
                        hwnd,
                        "EDIT",
                        &d.history_to,
                        WS_TABSTOP | ES_AUTOHSCROLL as u32,
                        [370, 12, 130, 28],
                        0,
                    );
                    let grouping = self.control(
                        hwnd,
                        "COMBOBOX",
                        "",
                        WS_TABSTOP | CBS_DROPDOWNLIST as u32,
                        [520, 12, 130, 180],
                        0,
                    );
                    for v in ["Day", "Week", "Month", "Quarter", "Year"] {
                        SendMessageW(
                            grouping,
                            CB_ADDSTRING,
                            0,
                            wide(&self.label(v, v)).as_ptr() as isize,
                        );
                    }
                    SendMessageW(
                        grouping,
                        CB_SETCURSEL,
                        match d.history_grouping {
                            HistoryGrouping::Day => 0,
                            HistoryGrouping::Week => 1,
                            HistoryGrouping::Month => 2,
                            HistoryGrouping::Quarter => 3,
                            HistoryGrouping::Year => 4,
                        },
                        0,
                    );
                    let log = self.control(
                        hwnd,
                        "BUTTON",
                        "Logarithmic chart",
                        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
                        [670, 12, 210, 28],
                        207,
                    );
                    SendMessageW(log, BM_SETCHECK, usize::from(o.history_log_scale), 0);
                    p.aux = vec![from, to, grouping, log];
                    self.control(
                        hwnd,
                        "BUTTON",
                        "Apply range",
                        WS_TABSTOP,
                        [20, 52, 130, 28],
                        204,
                    );
                    self.control(
                        hwnd,
                        "BUTTON",
                        "Import…",
                        WS_TABSTOP,
                        [170, 52, 110, 28],
                        205,
                    );
                    self.control(
                        hwnd,
                        "BUTTON",
                        "Export…",
                        WS_TABSTOP,
                        [300, 52, 110, 28],
                        206,
                    );
                    p.list = self.control(
                        hwnd,
                        "SysListView32",
                        "",
                        WS_TABSTOP | WS_VSCROLL | WS_HSCROLL | LVS_REPORT | LVS_SHOWSELALWAYS,
                        [20, 100, 600, 318],
                        0,
                    );
                    SendMessageW(
                        p.list,
                        LVM_SETEXTENDEDLISTVIEWSTYLE,
                        0,
                        (LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES | LVS_EX_DOUBLEBUFFER) as isize,
                    );
                    for (index, (label, width)) in [
                        ("Date / period", 110),
                        ("Upload", 105),
                        ("Download", 105),
                        ("Mixed / unknown", 135),
                        ("Total", 120),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let mut text = wide(&self.label(label, label));
                        let column = LVCOLUMNW {
                            mask: LVCF_WIDTH | LVCF_TEXT | LVCF_SUBITEM,
                            cx: width * dpi / 96,
                            pszText: text.as_mut_ptr(),
                            iSubItem: index as i32,
                            ..zeroed()
                        };
                        SendMessageW(
                            p.list,
                            LVM_INSERTCOLUMNW,
                            index,
                            &column as *const _ as isize,
                        );
                    }
                    self.populate_history(p.list, &d.history);
                    let controls = INITCOMMONCONTROLSEX {
                        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                        dwICC: ICC_DATE_CLASSES,
                    };
                    InitCommonControlsEx(&controls);
                    let calendar = self.control(
                        hwnd,
                        "SysMonthCal32",
                        "",
                        WS_TABSTOP,
                        [640, 128, 260, 215],
                        214,
                    );
                    p.aux.push(calendar);
                    self.control(
                        hwnd,
                        "STATIC",
                        "Click a date to select a single day.",
                        0,
                        [640, 355, 260, 40],
                        0,
                    );
                    p.detail = self.control(
                        hwnd,
                        "EDIT",
                        &d.history_summary,
                        ES_READONLY as u32,
                        [20, 430, 880, 28],
                        0,
                    );
                }
                Page::Network => {
                    self.control(hwnd,"STATIC","Select one or more adapters. Automatic mode avoids duplicate virtual/filter interfaces.",0,[20,15,880,45],0);
                    p.list = self.control(
                        hwnd,
                        "LISTBOX",
                        "",
                        WS_TABSTOP | WS_VSCROLL | LBS_EXTENDEDSEL as u32 | LBS_NOTIFY as u32,
                        [20, 65, 420, 490],
                        213,
                    );
                    for (i, n) in d.networks.iter().enumerate() {
                        SendMessageW(p.list, LB_ADDSTRING, 0, wide(&n.name).as_ptr() as isize);
                        if n.selected {
                            SendMessageW(p.list, LB_SETSEL, 1, i as isize);
                        }
                    }
                    p.detail = self.control(
                        hwnd,
                        "EDIT",
                        "Select an adapter to inspect addresses, counters and status.",
                        WS_VSCROLL | ES_READONLY as u32 | ES_MULTILINE as u32,
                        [460, 65, 440, 490],
                        0,
                    );
                    let mode = self.control(
                        hwnd,
                        "COMBOBOX",
                        "",
                        WS_TABSTOP | CBS_DROPDOWNLIST as u32,
                        [20, 580, 250, 150],
                        0,
                    );
                    for v in [
                        "Automatic active adapter",
                        "All eligible adapters",
                        "Selected adapters",
                    ] {
                        SendMessageW(
                            mode,
                            CB_ADDSTRING,
                            0,
                            wide(&self.label(v, v)).as_ptr() as isize,
                        );
                    }
                    SendMessageW(
                        mode,
                        CB_SETCURSEL,
                        match d.connection_mode {
                            ConnectionMode::Auto => 0,
                            ConnectionMode::All => 1,
                            ConnectionMode::Manual => 2,
                        },
                        0,
                    );
                    p.aux.push(mode);
                    self.control(
                        hwnd,
                        "BUTTON",
                        "Apply connection selection",
                        WS_TABSTOP,
                        [300, 580, 260, 30],
                        208,
                    );
                }
                Page::Plugins => {
                    p.list = self.control(
                        hwnd,
                        "LISTBOX",
                        "",
                        WS_TABSTOP | WS_VSCROLL | LBS_NOTIFY as u32,
                        [20, 20, 400, 470],
                        213,
                    );
                    for plugin in &d.plugins {
                        SendMessageW(
                            p.list,
                            LB_ADDSTRING,
                            0,
                            wide(&format!(
                                "{}  {}  {}",
                                if plugin.enabled {
                                    "[enabled]"
                                } else {
                                    "[disabled]"
                                },
                                plugin.name,
                                plugin.version
                            ))
                            .as_ptr() as isize,
                        );
                    }
                    SendMessageW(p.list, LB_SETCURSEL, 0, 0);
                    p.detail = self.control(
                        hwnd,
                        "EDIT",
                        "",
                        ES_READONLY as u32 | ES_MULTILINE as u32 | WS_VSCROLL,
                        [440, 20, 460, 350],
                        0,
                    );
                    let commands = self.control(
                        hwnd,
                        "COMBOBOX",
                        "",
                        WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
                        [440, 400, 460, 230],
                        0,
                    );
                    p.aux.push(commands);
                    self.control(
                        hwnd,
                        "BUTTON",
                        "Configure…",
                        WS_TABSTOP,
                        [20, 530, 160, 32],
                        209,
                    );
                    self.control(
                        hwnd,
                        "BUTTON",
                        "Enable / disable",
                        WS_TABSTOP,
                        [200, 530, 180, 32],
                        210,
                    );
                    self.control(
                        hwnd,
                        "BUTTON",
                        "Run selected command",
                        WS_TABSTOP,
                        [440, 450, 260, 32],
                        211,
                    );
                }
            }
        }
        let mut client: RECT = zeroed();
        GetClientRect(hwnd, &mut client);
        for (bar, max, page) in [
            (SB_VERT, 700 * dpi / 96, client.bottom),
            (SB_HORZ, 920 * dpi / 96, client.right),
        ] {
            let info = SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                nMin: 0,
                nMax: max,
                nPage: page as u32,
                nPos: 0,
                nTrackPos: 0,
            };
            if settings_tab.is_none() {
                SetScrollInfo(hwnd, bar, &info, 1);
            }
        }
        *self.panel.borrow_mut() = Some(p);
        if let Some(index) = settings_tab {
            self.resize_settings_panel();
            self.select_settings_tab(index);
            self.update_fixed_width_control();
        }
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        self.selection_details();
    }
    unsafe fn finish_preferences_apply(&self, success: bool) {
        let result = {
            let mut panel = self.panel.borrow_mut();
            if let Some(p) = panel.as_mut() {
                if let Some(s) = p.settings.as_mut() {
                    s.pending.take().map(|values| {
                        let language_changed = values
                            .iter()
                            .find(|(key, _)| key == "general/language")
                            .is_some_and(|entry| !s.baseline.contains(entry));
                        if success {
                            s.baseline = values;
                        }
                        let close = success && s.close_after_apply;
                        s.close_after_apply = false;
                        (
                            p.hwnd,
                            close,
                            success && language_changed,
                            SendMessageW(s.tab, TCM_GETCURSEL, 0, 0).max(0) as usize,
                        )
                    })
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some((hwnd, close, translate, index)) = result {
            EnableWindow(hwnd, 1);
            if close {
                self.close_panel();
            } else if translate {
                self.close_panel();
                self.open_panel(Page::Settings);
                self.select_settings_tab(index);
            }
        }
    }
    unsafe fn apply_panel(&self) -> bool {
        if self
            .panel
            .borrow()
            .as_ref()
            .and_then(|p| p.settings.as_ref())
            .is_some_and(|s| s.pending.is_some())
        {
            return false;
        }
        let Some((fields, baseline)) = self.panel.borrow().as_ref().map(|p| {
            (
                p.fields.clone(),
                p.settings
                    .as_ref()
                    .map(|s| s.baseline.clone())
                    .unwrap_or_default(),
            )
        }) else {
            return false;
        };
        let mut values = read_fields(&fields);
        let selection = self.panel.borrow().as_ref().and_then(|p| {
            p.settings.as_ref().map(|s| {
                (
                    picker_selected(&s.main_items),
                    picker_selected(&s.taskbar_items),
                    s.appearance,
                )
            })
        });
        if let Some((main, taskbar, appearance)) = selection {
            if self.options.borrow().skin.is_none() {
                if main.is_empty() {
                    self.error(&self.label(
                        "Select at least one floating display item",
                        "Select at least one floating display item",
                    ));
                    return false;
                }
                values.push(("visible_items".into(), main.join(",")));
            }
            values.push(("tb_visible_items".into(), taskbar.join(",")));
            let d = self.dashboard.borrow();
            let si = SendMessageW(appearance.0, CB_GETCURSEL, 0, 0);
            let li = SendMessageW(appearance.1, CB_GETCURSEL, 0, 0);
            let selected_skin = d.skins.get(si as usize).map(|s| s.id.clone());
            let selected_language = d.languages.get(li as usize).map(|l| l.id.clone());
            drop(d);
            if let Some(value) = selected_skin {
                values.push(("config/skin_selected".into(), value));
            }
            if let Some(value) = selected_language {
                values.push(("general/language".into(), value));
            }
        }
        let changed = changed_values(&values, &baseline);
        if changed.is_empty() {
            return true;
        }
        let validation = validate_fields(&changed, &self.dashboard.borrow().settings);
        if let Err(error) = validation {
            self.error(&error);
            return false;
        }
        let mut options = self.options.borrow().clone();
        if let Err(e) =
            apply_values(&mut options, &changed).and_then(|()| validate_options(&options))
        {
            self.error(&e);
            return false;
        }
        let auto_start = changed
            .iter()
            .find(|(key, _)| key == "_auto_start")
            .and_then(|(_, value)| {
                let enabled = value == "true";
                self.dashboard
                    .borrow()
                    .auto_start_enabled
                    .filter(|previous| *previous != enabled)
                    .map(|_| enabled)
            });
        let skin = changed
            .iter()
            .find(|(key, _)| key == "config/skin_selected")
            .map(|(_, value)| value.clone());
        let language = changed
            .iter()
            .find(|(key, _)| key == "general/language")
            .map(|(_, value)| value.clone());
        let fields: Vec<_> = changed
            .into_iter()
            .filter(|(key, _)| {
                key.contains('/') && key != "config/skin_selected" && key != "general/language"
            })
            .collect();
        if let Some(s) = self
            .panel
            .borrow_mut()
            .as_mut()
            .and_then(|p| p.settings.as_mut())
        {
            s.pending = Some(values);
        }
        if let Some(hwnd) = self.panel.borrow().as_ref().map(|p| p.hwnd) {
            EnableWindow(hwnd, 0);
        }
        self.queue(DesktopAction::ApplyPreferences {
            options,
            fields,
            skin,
            language,
            auto_start,
        });
        true
    }
    unsafe fn history_range(&self) {
        let aux = self.panel.borrow().as_ref().map(|p| p.aux.clone());
        if let Some(aux) = aux {
            let index = SendMessageW(aux[2], CB_GETCURSEL, 0, 0);
            self.queue(DesktopAction::HistoryRange {
                from: window_text(aux[0]),
                to: window_text(aux[1]),
                grouping: match index {
                    1 => HistoryGrouping::Week,
                    2 => HistoryGrouping::Month,
                    3 => HistoryGrouping::Quarter,
                    4 => HistoryGrouping::Year,
                    _ => HistoryGrouping::Day,
                },
            });
            PostMessageW(self.main.get(), WM_APP + 3, 0, 0);
        }
    }
    unsafe fn history_file(&self, save: bool) {
        let mut file = [0u16; 32768];
        let filter =
            wide("Traffic history (*.dat;*.txt;*.csv)\0*.dat;*.txt;*.csv\0All files\0*.*\0");
        let mut dialog: OPENFILENAMEW = zeroed();
        dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        dialog.hwndOwner = self
            .panel
            .borrow()
            .as_ref()
            .map_or(self.main.get(), |p| p.hwnd);
        dialog.lpstrFilter = filter.as_ptr();
        dialog.lpstrFile = file.as_mut_ptr();
        dialog.nMaxFile = file.len() as u32;
        dialog.Flags = OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            };
        dialog.lpstrDefExt = windows_sys::core::w!("dat");
        let accepted = if save {
            GetSaveFileNameW(&mut dialog)
        } else {
            GetOpenFileNameW(&mut dialog)
        };
        if accepted != 0 {
            let end = file.iter().position(|v| *v == 0).unwrap_or(file.len());
            let path = PathBuf::from(String::from_utf16_lossy(&file[..end]));
            self.queue(if save {
                DesktopAction::ExportHistory(path)
            } else {
                DesktopAction::ImportHistory(path)
            });
        }
    }
    unsafe fn network_select(&self) {
        let snapshot = self
            .panel
            .borrow()
            .as_ref()
            .map(|p| (p.list, p.aux.clone()));
        if let Some((list, aux)) = snapshot {
            let d = self.dashboard.borrow().clone();
            let ids = d
                .networks
                .iter()
                .enumerate()
                .filter(|(i, _)| SendMessageW(list, LB_GETSEL, *i, 0) > 0)
                .map(|(_, n)| n.id.clone())
                .collect();
            let mode = match SendMessageW(aux[0], CB_GETCURSEL, 0, 0) {
                0 => ConnectionMode::Auto,
                1 => ConnectionMode::All,
                _ => ConnectionMode::Manual,
            };
            self.queue(DesktopAction::SelectConnections { mode, ids });
        }
    }
    unsafe fn selected_plugin(&self) -> Option<PluginInfo> {
        let list = self.panel.borrow().as_ref()?.list;
        let index = SendMessageW(list, LB_GETCURSEL, 0, 0);
        if index < 0 {
            return None;
        }
        self.dashboard.borrow().plugins.get(index as usize).cloned()
    }
    unsafe fn plugin_configure(&self) {
        if let Some(p) = self.selected_plugin() {
            self.queue(DesktopAction::PluginConfigure(p.id));
        }
    }
    unsafe fn plugin_toggle(&self) {
        if let Some(p) = self.selected_plugin() {
            self.queue(DesktopAction::PluginEnabled {
                id: p.id,
                enabled: !p.enabled,
            });
        }
    }
    unsafe fn plugin_command(&self) {
        if let Some(p) = self.selected_plugin() {
            let h = self
                .panel
                .borrow()
                .as_ref()
                .and_then(|p| p.aux.first().copied());
            if let Some(h) = h {
                let index = SendMessageW(h, CB_GETCURSEL, 0, 0);
                if let Some((command, _)) = p.commands.get(index as usize) {
                    self.queue(DesktopAction::PluginCommand {
                        id: p.id,
                        command: *command,
                    });
                }
            }
        }
    }
    unsafe fn apply_appearance(&self) {
        let aux = self.panel.borrow().as_ref().map(|p| p.aux.clone());
        if let Some(aux) = aux {
            let d = self.dashboard.borrow().clone();
            let skin = SendMessageW(aux[0], CB_GETCURSEL, 0, 0);
            let language = SendMessageW(aux[1], CB_GETCURSEL, 0, 0);
            if let Some(s) = d.skins.get(skin as usize) {
                if s.id != d.current_skin {
                    self.queue(DesktopAction::SelectSkin(s.id.clone()));
                }
            }
            if let Some(s) = d.languages.get(language as usize) {
                if s.id != d.current_language {
                    self.queue(DesktopAction::SelectLanguage(s.id.clone()));
                }
            }
        }
    }
    unsafe fn selection_details(&self) {
        let snapshot = self
            .panel
            .borrow()
            .as_ref()
            .map(|p| (p.page, p.list, p.detail, p.aux.clone()));
        let Some((page, list, detail, aux)) = snapshot else {
            return;
        };
        if page == Page::Network {
            let index = SendMessageW(list, LB_GETCARETINDEX, 0, 0);
            let n = self
                .dashboard
                .borrow()
                .networks
                .get(index as usize)
                .cloned();
            if let Some(n) = n {
                let text = n
                    .details
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}"))
                    .collect::<Vec<_>>()
                    .join("\r\n");
                SetWindowTextW(detail, wide(&text).as_ptr());
            }
        } else if page == Page::Plugins {
            if let Some(p) = self.selected_plugin() {
                SetWindowTextW(
                    detail,
                    wide(&format!(
                        "{}\r\nVersion: {}\r\n{}\r\n{}",
                        p.name, p.version, p.id, p.description
                    ))
                    .as_ptr(),
                );
                SendMessageW(aux[0], CB_RESETCONTENT, 0, 0);
                for (_, text) in p.commands {
                    SendMessageW(aux[0], CB_ADDSTRING, 0, wide(&text).as_ptr() as isize);
                }
                SendMessageW(aux[0], CB_SETCURSEL, 0, 0);
            }
        }
    }
}
unsafe fn read_fields(fields: &[(String, HWND, SettingKind)]) -> Vec<(String, String)> {
    fields
        .iter()
        .map(|(key, hwnd, kind)| {
            let value = match kind {
                SettingKind::Bool => {
                    (SendMessageW(*hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED as isize).to_string()
                }
                SettingKind::Choice(choices) => choices
                    .get(SendMessageW(*hwnd, CB_GETCURSEL, 0, 0) as usize)
                    .cloned()
                    .unwrap_or_default(),
                _ => window_text(*hwnd),
            };
            (key.clone(), value)
        })
        .collect()
}
unsafe fn picker_selected(picker: &ItemPicker) -> Vec<String> {
    picker
        .entries
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            SendMessageW(
                picker.list,
                LVM_GETITEMSTATE,
                *i,
                LVIS_STATEIMAGEMASK as isize,
            ) >> 12
                == 2
        })
        .map(|(_, (id, _))| id.clone())
        .collect()
}
unsafe fn window_text(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd).max(0) as usize;
    let mut text = vec![0u16; len + 1];
    let read = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
    String::from_utf16_lossy(&text[..read.max(0) as usize])
}
fn format_color(color: u32) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        color & 255,
        (color >> 8) & 255,
        (color >> 16) & 255
    )
}
fn uses_png(o: &DesktopOptions) -> bool {
    o.skin
        .as_ref()
        .and_then(|s| s.background.as_ref())
        .and_then(|p| p.extension())
        .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("png"))
}
fn mask_opaque(argb: u32) -> bool {
    ((argb & 255) + ((argb >> 8) & 255) + ((argb >> 16) & 255)) / 3 > 128
}
unsafe fn draw_text(
    dc: HDC,
    surface: Option<&DibSurface>,
    text: &str,
    r: &mut RECT,
    flags: u32,
    mode: &TextMode,
) {
    if let Some(surface) = surface {
        surface.text(dc, text, r, flags, GetTextColor(dc));
        return;
    }
    if mode.directwrite {
        match crate::color_text::draw(
            dc as usize,
            text,
            [r.left, r.top, r.right, r.bottom],
            &mode.font,
            mode.points,
            mode.style,
            GetTextColor(dc),
            if flags & DT_RIGHT != 0 {
                1
            } else if flags & DT_CENTER != 0 {
                2
            } else {
                0
            },
            mode.colorful,
            mode.dpi,
        ) {
            Ok(()) => return,
            Err(e) => {
                if let Some(app) = current() {
                    if !app.renderer_warning.replace(true) {
                        let message =
                            format!("DirectWrite rendering failed ({e}); using GDI text fallback.");
                        eprintln!("{message}");
                        app.notify(&message);
                    }
                }
            }
        }
    }
    DrawTextW(dc, wide(text).as_ptr(), -1, r, flags);
}
fn double_click_name(action: DoubleClickAction) -> &'static str {
    match action {
        DoubleClickAction::Settings => "Settings",
        DoubleClickAction::History => "History",
        DoubleClickAction::Network => "Network",
        DoubleClickAction::TaskManager => "Task Manager",
        DoubleClickAction::ToggleMain => "Toggle floating monitor",
        DoubleClickAction::ToggleMore => "Show more information",
        DoubleClickAction::SpecificApp => "Open configured application",
        DoubleClickAction::ChangeSkin => "Next skin",
        DoubleClickAction::None => "Do nothing",
    }
}
fn double_click_choices() -> Vec<String> {
    [
        DoubleClickAction::Settings,
        DoubleClickAction::History,
        DoubleClickAction::Network,
        DoubleClickAction::TaskManager,
        DoubleClickAction::ToggleMore,
        DoubleClickAction::SpecificApp,
        DoubleClickAction::ChangeSkin,
        DoubleClickAction::None,
    ]
    .into_iter()
    .map(|a| double_click_name(a).into())
    .collect()
}
fn parse_double_click(value: &str) -> DoubleClickAction {
    match value {
        "History" => DoubleClickAction::History,
        "Network" => DoubleClickAction::Network,
        "Task Manager" => DoubleClickAction::TaskManager,
        "Toggle floating monitor" => DoubleClickAction::ToggleMain,
        "Show more information" => DoubleClickAction::ToggleMore,
        "Open configured application" => DoubleClickAction::SpecificApp,
        "Next skin" => DoubleClickAction::ChangeSkin,
        "Do nothing" => DoubleClickAction::None,
        _ => DoubleClickAction::Settings,
    }
}
fn parse_color(text: &str) -> Result<u32, String> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 {
        return Err("Color must contain six hexadecimal digits: #RRGGBB".into());
    }
    let rgb = u32::from_str_radix(digits, 16).map_err(|_| "Color must be #RRGGBB".to_string())?;
    if rgb > 0xffffff {
        return Err("Color must be #RRGGBB".into());
    }
    Ok(((rgb & 255) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 255))
}
unsafe fn system_dark_theme() -> bool {
    let mut light = 1u32;
    let mut size = 4u32;
    RegGetValueW(
        HKEY_CURRENT_USER,
        windows_sys::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        windows_sys::core::w!("AppsUseLightTheme"),
        RRF_RT_REG_DWORD,
        null_mut(),
        &mut light as *mut u32 as *mut c_void,
        &mut size,
    );
    light == 0
}
unsafe fn registry_dword(root: HKEY, path: *const u16, name: *const u16, fallback: u32) -> u32 {
    let mut value = fallback;
    let mut size = 4u32;
    RegGetValueW(
        root,
        path,
        name,
        RRF_RT_REG_DWORD,
        null_mut(),
        &mut value as *mut u32 as *mut c_void,
        &mut size,
    );
    value
}
unsafe fn is_windows11() -> bool {
    let mut build = [0u16; 32];
    let mut size = 64u32;
    RegGetValueW(
        HKEY_LOCAL_MACHINE,
        windows_sys::core::w!("Software\\Microsoft\\Windows NT\\CurrentVersion"),
        windows_sys::core::w!("CurrentBuildNumber"),
        RRF_RT_REG_SZ,
        null_mut(),
        build.as_mut_ptr().cast(),
        &mut size,
    );
    let len = build.iter().position(|v| *v == 0).unwrap_or(build.len());
    String::from_utf16_lossy(&build[..len])
        .parse::<u32>()
        .unwrap_or(0)
        >= 22000
}
fn apply_values(o: &mut DesktopOptions, values: &[(String, String)]) -> Result<(), String> {
    let original = o.clone();
    for (key, value) in values {
        let n = || {
            value
                .trim()
                .parse::<i32>()
                .map_err(|_| format!("{key} must be an integer"))
        };
        let b = value == "true";
        if let Some(bit) = key
            .strip_prefix("font_bit_")
            .and_then(|v| v.parse::<u8>().ok())
        {
            if b {
                o.font_style |= bit
            } else {
                o.font_style &= !bit
            }
            continue;
        }
        if let Some(bit) = key
            .strip_prefix("tb_font_bit_")
            .and_then(|v| v.parse::<u8>().ok())
        {
            if b {
                o.taskbar_style.font_style |= bit
            } else {
                o.taskbar_style.font_style &= !bit
            }
            continue;
        }
        match key.as_str() {
            "show_main" => o.show_main = b,
            "show_taskbar" => o.show_taskbar = b,
            "show_tray" => o.show_tray = b,
            "topmost" => o.topmost = b,
            "click_through" => o.click_through = b,
            "lock_position" => o.lock_position = b,
            "snap_to_edges" => o.snap_to_edges = b,
            "hide_fullscreen" => o.hide_fullscreen = b,
            "dark" => o.dark = b,
            "follow_system_theme" => o.follow_system_theme = b,
            "taskbar_two_rows" => o.taskbar_two_rows = b,
            "font_name" => o.font_name = value.clone(),
            "tb_font_name" => o.taskbar_style.font_name = value.clone(),
            "tb_auto_width" => o.taskbar_style.auto_width = b,
            "tb_font_size" => o.taskbar_style.font_size = n()?,
            "tb_font_style" => {
                o.taskbar_style.font_style =
                    u8::try_from(n()?).map_err(|_| "Font style must be 0–15")?
            }
            "font_style" => {
                o.font_style = u8::try_from(n()?).map_err(|_| "Font style must be 0–15")?
            }
            "tb_foreground" => o.taskbar_style.foreground = parse_color(value)?,
            "tb_background" => o.taskbar_style.background = parse_color(value)?,
            "tb_label_color" => {
                o.taskbar_style.label_color = if value.trim().is_empty() {
                    None
                } else {
                    Some(parse_color(value)?)
                }
            }
            "tb_value_color" => {
                o.taskbar_style.value_color = if value.trim().is_empty() {
                    None
                } else {
                    Some(parse_color(value)?)
                }
            }
            "tb_show_labels" => o.taskbar_style.show_labels = b,
            "tb_align_right" => o.taskbar_style.align_right = b,
            "tb_show_graph" => o.taskbar_style.show_graph = b,
            "tb_show_status_bar" => o.taskbar_style.show_status_bar = b,
            "tb_tooltip" => o.taskbar_style.tooltip = b,
            "tb_place_left" => o.taskbar_style.place_left = b,
            "tb_swap_up_down" => o.taskbar_style.swap_up_down = b,
            "tooltip" => o.tooltip = b,
            "swap_up_down" => o.swap_up_down = b,
            "tb_opacity" => {
                o.taskbar_style.opacity = u8::try_from(n()?).map_err(|_| "Opacity must be 0–255")?
            }
            "tb_top_offset" => o.taskbar_style.top_offset = n()?,
            "tb_vertical_margin" => o.taskbar_style.vertical_margin = n()?,
            "tb_graph_maximum" => {
                o.taskbar_style.graph_maximum = if value.trim().is_empty() {
                    None
                } else {
                    Some(
                        value
                            .parse::<f64>()
                            .map_err(|_| "Graph maximum must be a number")?,
                    )
                }
            }
            "tb_graph_color" => {
                o.taskbar_style.graph_color = if value.trim().is_empty() {
                    None
                } else {
                    Some(parse_color(value)?)
                }
            }
            "tb_transparent_color" => {
                o.taskbar_style.transparent_color = if value.trim().is_empty() {
                    None
                } else {
                    Some(parse_color(value)?)
                }
            }
            "tb_graph_style" => {
                o.taskbar_style.graph_style = if value == "History bars" {
                    GraphStyle::Bar
                } else {
                    GraphStyle::Line
                }
            }
            "tb_double_click" => o.taskbar_double_click = parse_double_click(value),
            "tb_visible_items" => {
                o.taskbar_style.visible_items = value
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned)
                    .collect()
            }
            "font_size" => o.font_size = n()?,
            "foreground" => o.foreground = parse_color(value)?,
            "background" => o.background = parse_color(value)?,
            "interval_ms" => {
                o.interval_ms = u32::try_from(n()?).map_err(|_| "Interval must be positive")?
            }
            "opacity" => o.opacity = u8::try_from(n()?).map_err(|_| "Opacity must be 0–255")?,
            "width" => o.width = n()?,
            "height" => o.height = n()?,
            "taskbar_width" => o.taskbar_width = n()?,
            "taskbar_offset" => o.taskbar_offset = n()?,
            "taskbar_monitor" => {
                o.taskbar_monitor =
                    usize::try_from(n()?).map_err(|_| "Monitor index must not be negative")?
            }
            "item_gap" => o.item_gap = n()?,
            "visible_items" => {
                o.visible_items = value
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned)
                    .collect()
            }
            "double_click" => {
                o.double_click = match value.as_str() {
                    "History" => DoubleClickAction::History,
                    "Network" => DoubleClickAction::Network,
                    "Task Manager" => DoubleClickAction::TaskManager,
                    "Toggle floating monitor" => DoubleClickAction::ToggleMain,
                    "Show more information" => DoubleClickAction::ToggleMore,
                    "Open configured application" => DoubleClickAction::SpecificApp,
                    "Next skin" => DoubleClickAction::ChangeSkin,
                    "Do nothing" => DoubleClickAction::None,
                    _ => DoubleClickAction::Settings,
                }
            }
            _ => {}
        }
    }
    if o.foreground != original.foreground {
        if let Some(skin) = o.skin.as_mut() {
            for item in &mut skin.items {
                item.color = Some(o.foreground);
            }
        }
    }
    if o.taskbar_style.foreground != original.taskbar_style.foreground {
        let color = Some(o.taskbar_style.foreground);
        if o.taskbar_style.label_color == original.taskbar_style.label_color {
            o.taskbar_style.label_color = color;
        }
        if o.taskbar_style.value_color == original.taskbar_style.value_color {
            o.taskbar_style.value_color = color;
        }
    }
    if o.taskbar_style.label_color != original.taskbar_style.label_color {
        for item in &mut o.taskbar_style.item_colors {
            item.label = o.taskbar_style.label_color;
        }
    }
    if o.taskbar_style.value_color != original.taskbar_style.value_color {
        for item in &mut o.taskbar_style.item_colors {
            item.value = o.taskbar_style.value_color;
        }
    }
    if !o.show_main && !o.show_taskbar && !o.show_tray {
        return Err("Keep at least one monitor or notification icon visible".into());
    }
    if o.click_through && !o.show_tray && !o.show_taskbar {
        return Err(
            "Click-through needs a notification icon or taskbar menu to recover the window".into(),
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    unsafe fn cancel_owned_common_dialog(app: &App, key: &str) -> bool {
        let root = app.panel.borrow().as_ref().unwrap().hwnd;
        let field = app
            .panel
            .borrow()
            .as_ref()
            .unwrap()
            .fields
            .iter()
            .find(|(k, _, _)| k == key)
            .unwrap()
            .1;
        let before = window_text(field);
        let thread = windows_sys::Win32::System::Threading::GetCurrentThreadId();
        let owner = root as usize;
        let closer = std::thread::spawn(move || {
            unsafe extern "system" fn close(hwnd: HWND, data: LPARAM) -> i32 {
                let state = &mut *(data as *mut (usize, bool));
                let mut class = [0u16; 32];
                let count = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
                if String::from_utf16_lossy(&class[..count.max(0) as usize]) == "#32770"
                    && GetWindow(hwnd, GW_OWNER) as usize == state.0
                {
                    state.1 = true;
                    PostMessageW(hwnd, WM_COMMAND, IDCANCEL as usize, 0);
                    return 0;
                }
                1
            }
            let mut state = (owner, false);
            for _ in 0..100 {
                EnumThreadWindows(thread, Some(close), &mut state as *mut _ as isize);
                if state.1 {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            false
        });
        app.choose_field(GetDlgCtrlID(field) as usize);
        closer.join().unwrap() && window_text(field) == before && app.actions.borrow().is_empty()
    }
    #[test]
    #[ignore = "explicit owned settings window acceptance; no production backend or registry writes"]
    fn owned_settings_tabs_preserve_edits_and_cancel_every_category() {
        use std::sync::{Arc, Mutex};
        type ObservedFlags = (bool, bool, bool, bool);
        struct Backend {
            observed: Arc<Mutex<Option<ObservedFlags>>>,
            sampled: bool,
        }
        impl DesktopBackend for Backend {
            fn options(&self) -> DesktopOptions {
                DesktopOptions {
                    show_tray: false,
                    interval_ms: 1000,
                    exit_after_ms: Some(200),
                    ..Default::default()
                }
            }
            fn sample(&mut self) -> Result<Dashboard, String> {
                if !self.sampled {
                    self.sampled = true;
                    if let Some(app) = current() {
                        unsafe {
                            app.open_panel(Page::Settings);
                            let root = app.panel.borrow().as_ref().unwrap().hwnd;
                            let tab = FindWindowExW(
                                root,
                                null_mut(),
                                wide("SysTabControl32").as_ptr(),
                                null(),
                            );
                            let field = app
                                .panel
                                .borrow()
                                .as_ref()
                                .unwrap()
                                .fields
                                .iter()
                                .find(|(k, _, _)| k == "interval_ms")
                                .unwrap()
                                .1;
                            SetWindowTextW(field, wide("1234").as_ptr());
                            if !tab.is_null() {
                                for index in [2, 4, 5, 0] {
                                    SendMessageW(tab, TCM_SETCURSEL, index, 0);
                                    let header = NMHDR {
                                        hwndFrom: tab,
                                        idFrom: 220,
                                        code: TCN_SELCHANGE,
                                    };
                                    app.message(root, WM_NOTIFY, 220, &header as *const _ as isize);
                                }
                            }
                            let retained = IsWindow(field) != 0 && window_text(field) == "1234";
                            let quiet = app.actions.borrow().is_empty();
                            app.close_panel();
                            app.open_panel(Page::Settings);
                            let original = app
                                .panel
                                .borrow()
                                .as_ref()
                                .unwrap()
                                .fields
                                .iter()
                                .find(|(k, _, _)| k == "interval_ms")
                                .unwrap()
                                .1;
                            let cancelled =
                                window_text(original) == "1000" && app.actions.borrow().is_empty();
                            app.close_panel();
                            *self.observed.lock().unwrap() =
                                Some((!tab.is_null(), retained, quiet, cancelled));
                        }
                    }
                }
                Ok(Dashboard::default())
            }
            fn dispatch(&mut self, _: DesktopAction) -> Result<(), String> {
                panic!("switching tabs and cancelling must not dispatch settings")
            }
        }
        let observed = Arc::new(Mutex::new(None));
        run(Box::new(Backend {
            observed: observed.clone(),
            sampled: false,
        }))
        .unwrap();
        assert_eq!(
            *observed.lock().unwrap(),
            Some((true, true, true, true)),
            "settings require native tabs, persistent edits, no switch saves, and full cancellation"
        );
    }
    #[test]
    #[ignore = "explicit owned tabbed editor, list modal, checkbox order and Apply acceptance"]
    fn owned_settings_checkbox_order_apply_and_modal_cancel() {
        use std::sync::{Arc, Mutex};
        struct Backend {
            observed: Arc<Mutex<Vec<bool>>>,
            sampled: bool,
        }
        impl DesktopBackend for Backend {
            fn options(&self) -> DesktopOptions {
                DesktopOptions {
                    show_tray: false,
                    exit_after_ms: Some(200),
                    ..Default::default()
                }
            }
            fn sample(&mut self) -> Result<Dashboard, String> {
                if !self.sampled {
                    self.sampled = true;
                    if let Some(app) = current() {
                        unsafe {
                            *app.dashboard.borrow_mut() = Dashboard {
                                auto_start_enabled: Some(false),
                                settings: vec![
                                    SettingField {
                                        key: "general/connections_hide".into(),
                                        label: "Hidden adapters".into(),
                                        value: "Ethernet".into(),
                                        kind: SettingKind::List,
                                        help: String::new(),
                                    },
                                    SettingField {
                                        key: "general/cpu_core_name".into(),
                                        label: "Select the CPU temperature to be monitored".into(),
                                        value: "Core Average".into(),
                                        kind: SettingKind::Text,
                                        help: String::new(),
                                    },
                                ],
                                ..Default::default()
                            };
                            app.open_panel(Page::Settings);
                            let general_page = app
                                .panel
                                .borrow()
                                .as_ref()
                                .and_then(|p| p.settings.as_ref())
                                .unwrap()
                                .pages[0]
                                .0;
                            let long_label = FindWindowExW(
                                general_page,
                                null_mut(),
                                wide("STATIC").as_ptr(),
                                wide("Select the CPU temperature to be monitored").as_ptr(),
                            );
                            let mut label_rect: RECT = zeroed();
                            GetWindowRect(long_label, &mut label_rect);
                            let label_dc = GetDC(long_label);
                            let old_font = SelectObject(label_dc, app.panel_font.get());
                            let text = wide("Select the CPU temperature to be monitored");
                            let mut text_size: SIZE = zeroed();
                            GetTextExtentPoint32W(
                                label_dc,
                                text.as_ptr(),
                                text.len() as i32 - 1,
                                &mut text_size,
                            );
                            SelectObject(label_dc, old_font);
                            ReleaseDC(long_label, label_dc);
                            let label_fits = !long_label.is_null()
                                && label_rect.right - label_rect.left >= text_size.cx;
                            let field = app
                                .panel
                                .borrow()
                                .as_ref()
                                .unwrap()
                                .fields
                                .iter()
                                .find(|(k, _, _)| k == "general/connections_hide")
                                .unwrap()
                                .1;
                            app.choose_field(GetDlgCtrlID(field) as usize);
                            let root = app.panel.borrow().as_ref().unwrap().hwnd;
                            let modal = IsWindowEnabled(root) == 0;
                            let entry = app.list_editor.borrow().as_ref().unwrap().entry;
                            SetWindowTextW(entry, wide("Wi-Fi").as_ptr());
                            app.list_edit_add();
                            app.command(IDCANCEL as usize);
                            let list_cancelled =
                                window_text(field) == "Ethernet" && IsWindowEnabled(root) != 0;
                            app.choose_field(GetDlgCtrlID(field) as usize);
                            let entry = app.list_editor.borrow().as_ref().unwrap().entry;
                            SetWindowTextW(entry, wide("Wi-Fi").as_ptr());
                            app.list_edit_add();
                            app.close_list_editor(true);
                            let list_staged = window_text(field) == "Ethernet,Wi-Fi"
                                && app.actions.borrow().is_empty();
                            let font_cancelled = cancel_owned_common_dialog(&app, "font_name");
                            let color_cancelled = cancel_owned_common_dialog(&app, "foreground");
                            let (list, main_before) = app
                                .panel
                                .borrow()
                                .as_ref()
                                .and_then(|p| p.settings.as_ref())
                                .map(|s| (s.taskbar_items.list, picker_selected(&s.main_items)))
                                .unwrap();
                            if main_before.is_empty() {
                                app.close_panel();
                                *self.observed.lock().unwrap() = vec![false];
                                return Ok(Dashboard::default());
                            }
                            let count = SendMessageW(list, LVM_GETITEMCOUNT, 0, 0);
                            let recommend = GetDlgItem(GetParent(list), 234);
                            SendMessageW(recommend, BM_CLICK, 0, 0);
                            let recommended_staged = !recommend.is_null()
                                && app.actions.borrow().is_empty()
                                && app.options.borrow().taskbar_style.visible_items.is_empty()
                                && app
                                    .panel
                                    .borrow()
                                    .as_ref()
                                    .and_then(|p| p.settings.as_ref())
                                    .is_some_and(|s| {
                                        picker_selected(&s.taskbar_items)
                                            == vec!["up", "down", "cpu", "memory"]
                                            && picker_selected(&s.main_items) == main_before
                                    });
                            for i in 0..count {
                                let item = LVITEMW {
                                    stateMask: LVIS_STATEIMAGEMASK,
                                    state: 1 << 12,
                                    ..zeroed()
                                };
                                SendMessageW(
                                    list,
                                    LVM_SETITEMSTATE,
                                    i as usize,
                                    &item as *const _ as isize,
                                );
                            }
                            for i in [0, 1] {
                                let item = LVITEMW {
                                    stateMask: LVIS_STATEIMAGEMASK | LVIS_SELECTED,
                                    state: (2 << 12) | if i == 1 { LVIS_SELECTED } else { 0 },
                                    ..zeroed()
                                };
                                SendMessageW(list, LVM_SETITEMSTATE, i, &item as *const _ as isize);
                            }
                            app.move_display_item(232);
                            let independent = app
                                .panel
                                .borrow()
                                .as_ref()
                                .and_then(|p| p.settings.as_ref())
                                .is_some_and(|s| {
                                    picker_selected(&s.main_items) == main_before
                                        && picker_selected(&s.taskbar_items) == vec!["down", "up"]
                                });
                            let applied = app.apply_panel();
                            let actions: Vec<_> = app.actions.borrow_mut().drain(..).collect();
                            let pending_baseline = app
                                .panel
                                .borrow()
                                .as_ref()
                                .and_then(|p| p.settings.as_ref())
                                .is_some_and(|s| {
                                    s.pending.is_some()
                                        && s.baseline.iter().any(|(k, v)| {
                                            k == "general/connections_hide" && v == "Ethernet"
                                        })
                                });
                            let explicit_apply=applied && actions.len()==1 && actions.iter().any(|a|matches!(a,DesktopAction::ApplyPreferences{options,fields,auto_start,..}
                            if options.taskbar_style.visible_items==vec!["down","up"] && fields==&vec![("general/connections_hide".into(),"Ethernet,Wi-Fi".into())] && auto_start.is_none()));
                            for action in &actions {
                                if let DesktopAction::ApplyPreferences { options, .. } = action {
                                    *app.options.borrow_mut() = options.clone();
                                }
                            }
                            app.finish_preferences_apply(true);
                            app.apply_panel();
                            let repeat_quiet = app.actions.borrow().is_empty();
                            for i in 0..count {
                                let item = LVITEMW {
                                    stateMask: LVIS_STATEIMAGEMASK,
                                    state: 1 << 12,
                                    ..zeroed()
                                };
                                SendMessageW(
                                    list,
                                    LVM_SETITEMSTATE,
                                    i as usize,
                                    &item as *const _ as isize,
                                );
                            }
                            app.apply_panel();
                            let clear_all=app.actions.borrow_mut().drain(..).any(|a|matches!(a,DesktopAction::ApplyPreferences{options,..} if options.taskbar_style.visible_items.is_empty()));
                            app.finish_preferences_apply(false);
                            app.apply_panel();
                            let retry_after_failure=app.actions.borrow_mut().drain(..).any(|a|matches!(a,DesktopAction::ApplyPreferences{options,..} if options.taskbar_style.visible_items.is_empty()));
                            app.finish_preferences_apply(true);
                            app.close_panel();
                            *self.observed.lock().unwrap() = vec![
                                modal,
                                list_cancelled,
                                list_staged,
                                count == 13,
                                independent,
                                explicit_apply,
                                repeat_quiet,
                                clear_all,
                                font_cancelled,
                                color_cancelled,
                                pending_baseline,
                                retry_after_failure,
                                label_fits,
                                recommended_staged,
                            ];
                        }
                    }
                }
                Ok(Dashboard::default())
            }
            fn dispatch(&mut self, _: DesktopAction) -> Result<(), String> {
                panic!("test must consume all staged actions")
            }
        }
        let observed = Arc::new(Mutex::new(vec![]));
        run(Box::new(Backend {
            observed: observed.clone(),
            sampled: false,
        }))
        .unwrap();
        assert_eq!(
            *observed.lock().unwrap(),
            vec![true; 14],
            "modal ownership, Cancel, staged list, 13 checkboxes, independent order, explicit Apply, repeat Apply, all-clear and font/color dialog Cancel"
        );
    }
    #[test]
    #[ignore = "explicit owned appearance selectors can Apply and return to the opening values"]
    fn owned_settings_appearance_can_apply_then_restore_original_choices() {
        use std::sync::{Arc, Mutex};
        type SavedFields = Vec<(String, String)>;
        struct Backend {
            observed: Arc<Mutex<Vec<SavedFields>>>,
            sampled: bool,
        }
        impl DesktopBackend for Backend {
            fn options(&self) -> DesktopOptions {
                DesktopOptions {
                    show_tray: false,
                    exit_after_ms: Some(200),
                    ..Default::default()
                }
            }
            fn sample(&mut self) -> Result<Dashboard, String> {
                if !self.sampled {
                    self.sampled = true;
                    if let Some(app) = current() {
                        unsafe {
                            *app.dashboard.borrow_mut() = Dashboard {
                                current_skin: "A".into(),
                                current_language: "en".into(),
                                skins: vec![
                                    Choice {
                                        id: "A".into(),
                                        name: "A".into(),
                                    },
                                    Choice {
                                        id: "B".into(),
                                        name: "B".into(),
                                    },
                                ],
                                languages: vec![
                                    Choice {
                                        id: "en".into(),
                                        name: "English".into(),
                                    },
                                    Choice {
                                        id: "zh".into(),
                                        name: "中文".into(),
                                    },
                                ],
                                ..Default::default()
                            };
                            app.open_panel(Page::Appearance);
                            for index in [1, 0] {
                                let old_root = app.panel.borrow().as_ref().unwrap().hwnd;
                                let (skin, language) = app
                                    .panel
                                    .borrow()
                                    .as_ref()
                                    .and_then(|p| p.settings.as_ref())
                                    .unwrap()
                                    .appearance;
                                SendMessageW(skin, CB_SETCURSEL, index, 0);
                                SendMessageW(language, CB_SETCURSEL, index, 0);
                                app.apply_panel();
                                let mut batch = vec![];
                                for action in app.actions.borrow_mut().drain(..) {
                                    if let DesktopAction::ApplyPreferences {
                                        skin, language, ..
                                    } = action
                                    {
                                        if let Some(skin) = skin {
                                            app.dashboard.borrow_mut().current_skin = skin.clone();
                                            batch.push(("config/skin_selected".into(), skin));
                                        }
                                        if let Some(language) = language {
                                            app.dashboard.borrow_mut().current_language =
                                                language.clone();
                                            batch.push(("general/language".into(), language));
                                        }
                                    }
                                }
                                app.finish_preferences_apply(true);
                                batch.push((
                                    "_locale_controls_rebuilt".into(),
                                    (old_root != app.panel.borrow().as_ref().unwrap().hwnd)
                                        .to_string(),
                                ));
                                self.observed.lock().unwrap().push(batch);
                            }
                            app.close_panel();
                        }
                    }
                }
                Ok(Dashboard::default())
            }
            fn dispatch(&mut self, _: DesktopAction) -> Result<(), String> {
                panic!("test consumes its actions")
            }
        }
        let observed = Arc::new(Mutex::new(vec![]));
        run(Box::new(Backend {
            observed: observed.clone(),
            sampled: false,
        }))
        .unwrap();
        assert_eq!(
            *observed.lock().unwrap(),
            vec![
                vec![
                    ("config/skin_selected".into(), "B".into()),
                    ("general/language".into(), "zh".into()),
                    ("_locale_controls_rebuilt".into(), "true".into())
                ],
                vec![
                    ("config/skin_selected".into(), "A".into()),
                    ("general/language".into(), "en".into()),
                    ("_locale_controls_rebuilt".into(), "true".into())
                ]
            ]
        );
    }
    #[test]
    fn mask_threshold_matches_legacy_luminance() {
        assert!(!mask_opaque(0xff000000));
        assert!(!mask_opaque(0xff808080));
        assert!(mask_opaque(0xff818181));
        assert!(mask_opaque(0xffffffff));
    }
    #[test]
    fn png_surface_retains_partial_alpha_and_draws_black_text() {
        unsafe {
            let input = GdiplusStartupInput {
                GdiplusVersion: 1,
                DebugEventCallback: 0,
                SuppressBackgroundThread: 0,
                SuppressExternalCodecs: 0,
            };
            let mut token = 0;
            assert_eq!(GdiplusStartup(&mut token, &input, null_mut()), 0);
            let source = [0x00000000u32, 0x80800000, 0xffff0000];
            let mut bitmap = null_mut();
            assert_eq!(
                GdipCreateBitmapFromScan0(
                    3,
                    1,
                    12,
                    0x000e200b,
                    source.as_ptr().cast(),
                    &mut bitmap
                ),
                0
            );
            let surface = DibSurface::new(null_mut(), 3, 1).unwrap();
            surface.draw_background(bitmap);
            let pixels = surface.pixels();
            assert_eq!(pixels[0] >> 24, 0);
            assert_eq!(pixels[1] >> 24, 128);
            assert_eq!(pixels[2] >> 24, 255);
            GdipDisposeImage(bitmap.cast());
            drop(surface);
            let surface = DibSurface::new(null_mut(), 200, 40).unwrap();
            let old = SelectObject(surface.dc, GetStockObject(DEFAULT_GUI_FONT));
            surface.text(
                surface.dc,
                "Black text on transparent PNG",
                &RECT {
                    left: 0,
                    top: 0,
                    right: 200,
                    bottom: 40,
                },
                DT_LEFT,
                0,
            );
            assert!(
                surface
                    .pixels()
                    .iter()
                    .any(|p| p >> 24 > 0 && p & 0xffffff == 0)
            );
            SelectObject(surface.dc, old);
            drop(surface);
            GdiplusShutdown(token);
        }
    }
    #[test]
    fn editing_unified_color_changes_rendered_skin_and_item_colors() {
        let mut o = DesktopOptions {
            skin: Some(SkinLayout {
                background: None,
                mask: None,
                width: 100,
                height: 30,
                items: vec![SkinItem {
                    id: "upload".into(),
                    rect: [0, 0, 100, 30],
                    color: Some(0x112233),
                    alignment: TextAlignment::Left,
                    align_right: false,
                    show_label: true,
                }],
            }),
            ..Default::default()
        };
        o.taskbar_style.item_colors = vec![ItemColors {
            id: "upload".into(),
            label: Some(0x112233),
            value: Some(0x445566),
        }];
        apply_values(
            &mut o,
            &[
                ("foreground".into(), "#123456".into()),
                ("tb_foreground".into(), "#ABCDEF".into()),
            ],
        )
        .unwrap();
        assert_eq!(o.skin.as_ref().unwrap().items[0].color, Some(0x563412));
        assert_eq!(o.taskbar_style.item_colors[0].label, Some(0xefcdab));
        assert_eq!(o.taskbar_style.item_colors[0].value, Some(0xefcdab));
    }
    #[test]
    fn color_roundtrip_and_strict_six_digits() {
        assert_eq!(parse_color("#123456").unwrap(), 0x563412);
        assert_eq!(format_color(0x563412), "#123456");
        assert!(parse_color("#123").is_err());
        assert!(parse_color("#1000000").is_err());
    }
    #[test]
    fn taskbar_settings_are_independent_of_main_style() {
        let mut o = DesktopOptions::default();
        apply_values(
            &mut o,
            &[
                ("tb_font_size".into(), "18".into()),
                ("tb_foreground".into(), "#123456".into()),
                ("tb_font_bit_1".into(), "true".into()),
                ("tb_visible_items".into(), "download, upload".into()),
            ],
        )
        .unwrap();
        assert_eq!(o.font_size, 14);
        assert_eq!(o.foreground, 0xeeeeee);
        assert_eq!(o.taskbar_style.font_size, 18);
        assert_eq!(o.taskbar_style.foreground, 0x563412);
        assert_eq!(o.taskbar_style.font_style, 1);
        assert_eq!(o.taskbar_style.visible_items, vec!["download", "upload"]);
    }
    #[test]
    fn options_cannot_remove_every_recovery_surface() {
        let mut o = DesktopOptions::default();
        assert!(
            apply_values(
                &mut o,
                &[
                    ("show_main".into(), "false".into()),
                    ("show_tray".into(), "false".into())
                ]
            )
            .is_err()
        );
    }
    #[test]
    fn wide_strings_are_nul_terminated_and_keep_non_bmp_text() {
        assert_eq!(wide("监控🦀").last(), Some(&0));
        assert_eq!(String::from_utf16(&wide("监控🦀")[..4]).unwrap(), "监控🦀");
    }
    #[test]
    #[ignore = "explicit interactive window lifecycle and nested-message-loop acceptance"]
    fn interactive_lifecycle_reentrant_backend_and_cleanup() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        struct Backend {
            samples: Arc<AtomicUsize>,
            shutdown: Arc<AtomicUsize>,
            options: DesktopOptions,
        }
        impl DesktopBackend for Backend {
            fn options(&self) -> DesktopOptions {
                self.options.clone()
            }
            fn sample(&mut self) -> Result<Dashboard, String> {
                let n = self.samples.fetch_add(1, Ordering::SeqCst);
                if let Some(app) = current() {
                    unsafe {
                        if n == 0 {
                            for page in [
                                Page::Settings,
                                Page::Taskbar,
                                Page::Advanced,
                                Page::History,
                                Page::Network,
                                Page::Appearance,
                                Page::Plugins,
                            ] {
                                app.open_panel(page);
                            }
                            PostMessageW(app.main.get(), REFRESH, 0, 0);
                            let mut msg = zeroed();
                            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                                TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                            }
                            app.close_panel();
                        }
                    }
                }
                Ok(Dashboard::default())
            }
            fn dispatch(&mut self, _: DesktopAction) -> Result<(), String> {
                Ok(())
            }
            fn shutdown(&mut self) -> Result<(), String> {
                self.shutdown.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
        let samples = Arc::new(AtomicUsize::new(0));
        let shutdown = Arc::new(AtomicUsize::new(0));
        run(Box::new(Backend {
            samples: samples.clone(),
            shutdown: shutdown.clone(),
            options: DesktopOptions {
                title: "TrafficMonitor lifecycle test".into(),
                show_tray: false,
                interval_ms: 100,
                exit_after_ms: Some(350),
                ..Default::default()
            },
        }))
        .unwrap();
        assert!(samples.load(Ordering::SeqCst) >= 2);
        assert_eq!(shutdown.load(Ordering::SeqCst), 1);
        assert!(current().is_none());
        unsafe {
            assert!(
                FindWindowW(
                    wide(CLASS).as_ptr(),
                    wide("TrafficMonitor lifecycle test").as_ptr()
                )
                .is_null()
            );
        }
    }
}

#[cfg(test)]
mod taskbar_acceptance {
    use super::*;
    #[test]
    #[ignore = "explicit interactive taskbar attachment, rebuild and restoration acceptance"]
    fn interactive_taskbar_rebuild_restores_explorer_geometry() {
        use std::sync::{Arc, Mutex};
        let before = traffic_monitor_shell::probe_taskbars().expect("interactive Explorer taskbar");
        struct Backend {
            options: DesktopOptions,
            handles: Arc<Mutex<Vec<(usize, usize)>>>,
            samples: usize,
        }
        impl DesktopBackend for Backend {
            fn options(&self) -> DesktopOptions {
                self.options.clone()
            }
            fn sample(&mut self) -> Result<Dashboard, String> {
                self.samples += 1;
                if self.samples == 2 {
                    if let Some(app) = current() {
                        unsafe {
                            PostMessageW(app.main.get(), app.taskbar_created, 0, 0);
                        }
                    }
                }
                Ok(Dashboard {
                    items: vec![DisplayValue {
                        id: "test".into(),
                        label: "UI test".into(),
                        value: "42".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                })
            }
            fn dispatch(&mut self, _: DesktopAction) -> Result<(), String> {
                Ok(())
            }
            fn attach_windows(&mut self, _: usize, taskbar: Option<usize>) {
                if let Some(h) = taskbar {
                    unsafe {
                        assert_ne!(IsWindow(h as HWND), 0);
                        assert!(!GetParent(h as HWND).is_null());
                    }
                    let mut v = self.handles.lock().unwrap();
                    let generation = current().map_or(0, |a| a.taskbar_generation.get());
                    if !v.iter().any(|(g, _)| *g == generation) {
                        v.push((generation, h));
                    }
                }
            }
        }
        let handles = Arc::new(Mutex::new(vec![]));
        run(Box::new(Backend {
            options: DesktopOptions {
                show_taskbar: true,
                show_tray: false,
                taskbar_style: TaskbarStyle {
                    visible_items: vec!["test".into()],
                    ..Default::default()
                },
                interval_ms: 100,
                exit_after_ms: Some(450),
                ..Default::default()
            },
            handles: handles.clone(),
            samples: 0,
        }))
        .unwrap();
        let handles = handles.lock().unwrap();
        assert!(
            handles.len() >= 2,
            "TaskbarCreated should rebuild the child"
        );
        for (_, h) in handles.iter() {
            unsafe {
                assert_eq!(IsWindow(*h as HWND), 0);
            }
        }
        let after = traffic_monitor_shell::probe_taskbars().unwrap();
        for b in before {
            if let Some(a) = after.iter().find(|a| a.taskbar.window == b.taskbar.window) {
                if let (Some(b), Some(a)) = (b.task_buttons, a.task_buttons.clone()) {
                    assert_eq!(
                        b.client_rect, a.client_rect,
                        "Task button geometry must be restored"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod session_acceptance {
    use super::*;
    #[test]
    #[ignore = "explicit owned-window logoff cancellation and checkpoint acceptance"]
    fn interactive_session_query_cancel_then_finish() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        struct Backend {
            samples: Arc<AtomicUsize>,
            checkpoints: Arc<AtomicUsize>,
            stops: Arc<AtomicUsize>,
        }
        impl DesktopBackend for Backend {
            fn options(&self) -> DesktopOptions {
                DesktopOptions {
                    title: "TrafficMonitor session lifecycle test".into(),
                    show_tray: false,
                    interval_ms: 100,
                    exit_after_ms: Some(2500),
                    ..Default::default()
                }
            }
            fn sample(&mut self) -> Result<Dashboard, String> {
                self.samples.fetch_add(1, Ordering::SeqCst);
                Ok(Dashboard::default())
            }
            fn dispatch(&mut self, _: DesktopAction) -> Result<(), String> {
                Ok(())
            }
            fn checkpoint(&mut self) -> Result<(), String> {
                self.checkpoints.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            fn shutdown(&mut self) -> Result<(), String> {
                self.stops.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
        let samples = Arc::new(AtomicUsize::new(0));
        let checkpoints = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let worker_samples = samples.clone();
        let worker = std::thread::spawn(move || unsafe {
            let mut hwnd = null_mut();
            for _ in 0..100 {
                hwnd = FindWindowW(
                    wide(CLASS).as_ptr(),
                    wide("TrafficMonitor session lifecycle test").as_ptr(),
                );
                if !hwnd.is_null() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(!hwnd.is_null());
            assert_eq!(SendMessageW(hwnd, WM_QUERYENDSESSION, 0, 0), 1);
            SendMessageW(hwnd, WM_ENDSESSION, 0, 0);
            let before = worker_samples.load(Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(350));
            assert!(
                worker_samples.load(Ordering::SeqCst) > before,
                "Canceled shutdown must keep sampling"
            );
            assert_eq!(SendMessageW(hwnd, WM_QUERYENDSESSION, 0, 0), 1);
            SendMessageW(hwnd, WM_ENDSESSION, 1, 0);
        });
        run(Box::new(Backend {
            samples,
            checkpoints: checkpoints.clone(),
            stops: stops.clone(),
        }))
        .unwrap();
        worker.join().unwrap();
        assert_eq!(checkpoints.load(Ordering::SeqCst), 2);
        assert_eq!(stops.load(Ordering::SeqCst), 1);
    }
}
