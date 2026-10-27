# Windows 任务栏适配原型（RM-13）

本 crate 提供任务栏宿主的只读快照、纯几何布局，以及显式触发的隐藏子窗口生命周期探针。它仍是适配原型，不能作为完整任务栏迁移或可见界面验收。

## 接口与边界

- `probe_taskbars() -> Result<Vec<TaskbarHost>, ProbeError>`：枚举当前桌面的 `Shell_TrayWnd` 和 `Shell_SecondaryTrayWnd`。按窗口类报告 `ReBarWindow32`/`WorkerW`、`MSTaskSwWClass`/`MSTaskListWClass`、`TrayNotifyWnd`、`Start`；可选子窗口不存在时返回 `None`，不会用空父句柄意外搜索其他顶层窗口。结果顺序是 Win32 枚举顺序，不表示主屏优先，也不依据这些子窗口推断 Windows 版本。
- 每个 `WindowSnapshot` 包含非拥有的 `WindowId`、类名、进程/线程 ID、screen/client/client-to-screen 三组矩形和 `GetDpiForWindow` 的有效 DPI。矩形右/下边界不包含在区域内。`client_screen_rect` 对映射后的两个屏幕点按各轴 min/max 规范成物理包围矩形，避免 `WS_EX_LAYOUTRTL` 镜像窗口左右角交换后产生倒置矩形；保留负屏幕坐标和空 client 尺寸。读取使用线程级 per-monitor DPI 上下文，返回前恢复原上下文；不更改进程或系统 DPI 设置。快照不是原子事务，Explorer 可随时销毁或复用句柄，动态变化需要重新探测。
- `taskbar_orientation(Rect)` 按宽高判定横向/纵向；`layout_panel(LayoutRequest)` 将 DIP 尺寸和边距按 DPI 四舍五入成像素，在调用方明确提供的可用区域内按起端/末端对齐，另一轴居中。最小非零尺寸保留一个像素。拒绝非法边界、零 DPI、空内容和空间不足，不裁小面板，也不自动认定任务按钮区域是空闲空间。
- `probe_hidden_child(&TaskbarHost)`：在该任务栏根窗口下创建禁用且不可见的自有 1×1 子窗口，核对父窗口和隐藏状态，随后销毁子窗口并注销私有类。使用和父窗口一致的线程 DPI 上下文，完成后恢复。RAII guard 具有线程亲和性，早退时也执行释放；正常路径显式检查销毁/注销结果。此探针尚未在经典 ReBar/WorkerW 容器下创建可见面板。

探针不移动或缩放 Explorer 宿主/任务按钮，不调用 `SetParent`，不写注册表，不改系统配置，不创建定时器、后台线程或常驻进程。隐藏窗口无 `WS_VISIBLE`，还设置 `WS_DISABLED`、`WS_EX_NOACTIVATE` 和 `WS_EX_NOPARENTNOTIFY`。回调使用正确的 `extern "system"` ABI，类/窗口始终由创建线程释放。`WindowId::value()` 仅用于诊断，不转移窗口所有权。

找不到任何任务栏时返回 `ExplorerUnavailable`，明确提示 Explorer 停止、CI/service 或非交互桌面等可能情况；不能把这个诊断当作真实宿主验收成功。枚举、读几何、创建和清理失败时保留操作名及对应 Win32 last-error；未定义 last-error 的零 DPI 返回 `HostChanged`。

Win10 1607 及后续 Win10/11 使用线程/窗口 DPI API；V2 不可用时尝试 per-monitor V1。API 动态解析，因此早期 Win10 不会因缺少这些导出而在进程加载时失败，但当前会明确返回 `UnsupportedDpiApi`。早期 Win10 的 DPI fallback 尚未实现。非 Windows 平台可使用纯布局；原生探针返回 `UnsupportedPlatform`。

## 验证命令

在仓库根目录执行。默认测试不读取 Explorer、不创建原生窗口，适用于无 Explorer 的 Windows CI 会话：

```powershell
& cargo test -p traffic-monitor-shell --locked --offline -j 1
& cargo fmt -p traffic-monitor-shell -- --check
& cargo clippy -p traffic-monitor-shell --locked --offline --all-targets -j 1 -- -D warnings
```

默认只读真机探测：

```powershell
& cargo run -p traffic-monitor-shell --example taskbar_probe --locked --offline -j 1
```

以下命令**显式**创建短暂的隐藏子窗口，退出前自动销毁，不显示可见界面：

```powershell
& cargo run -p traffic-monitor-shell --example taskbar_probe --locked --offline -j 1 -- --hidden-child
& cargo test -p traffic-monitor-shell --test windows_host --locked --offline -j 1 -- --ignored --test-threads=1
```

交互宿主测试默认 `#[ignore]`；显式执行时必须存在真实任务栏，不将宿主缺失当作跳过或通过。该测试检查线程 DPI 上下文恢复，并循环 8 次检查隐藏子窗口销毁及私有类注销。

2026-10-27 本机证据：Windows 11 专业版 `10.0.26300`，主任务栏 screen `(0,1392)-(2560,1440)`、client `2560×48`、DPI `96`；读到 ReBar、任务按钮、通知区和 Start。默认 21 个测试通过、2 个交互测试忽略；新增 RTL 屏幕矩形规范测试已观察到修复前倒置的红灯，再修复通过，另覆盖负/极值坐标及空 client 尺寸。另显式运行这 2 个交互测试均通过，隐藏生命周期为 `ClassRegistered → ChildCreated → HiddenChildVerified → ChildDestroyed → ClassUnregistered`。fmt 检查及 Clippy all-targets `-D warnings` 通过。当前桌面只观察到主任务栏；无 Explorer 的错误分类由纯内存测试覆盖，没有停止本机 Explorer 来制造该场景；未修改真实任务栏为 RTL，RTL 修复证据来自纯点映射回归。

## 后续验收

仍需覆盖 Windows 10 真机、副屏/多屏与混合 DPI、早期 Win10 fallback、经典容器的可见嵌入、横/竖任务栏真机、可见内容渲染和交互、实际空闲区域计算与原 C++ 布局行为一致性。动态多屏监听、自动隐藏、Explorer 重启后自动重建也尚未实现。本次隐藏子窗口验证不代替这些验收；未修改原 C++ 的任务按钮挤压/恢复逻辑。

原实现参照：`TaskBarDlg.cpp` 的 `FindTaskbarHandle`、`OnInitDialog`、`AdjustWindowPos`，`ClassicalTaskbarDlg.cpp` 的 `InitTaskbarWnd`/`GetParentHwnd`/`CheckTaskbarOnTopOrBottom`，以及 `Win11TaskbarDlg.cpp` 的 `InitTaskbarWnd`/`GetParentHwnd`。经典父容器与 Win11 根任务栏的区分来自这些代码；本原型保留相关观察数据，暂不自动选择可见嵌入策略。
