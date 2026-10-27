# TrafficMonitor 渐进式 Rust 重构设计

用户已确认采用 Rust，并要求在新分支逐步重构。工作分支为 `refactor/rust-migration`，起点为 `930f175`。

用户进一步确认重构版仅支持 Windows 10/11，Rust 接入默认启用；不维持 Windows 7/8 的新版本运行兼容。后续追加的全量清单与并发任务以 `docs/RUST_MIGRATION.md` 和根目录规划文件为准。

## 迁移方式

采用 Rust 核心与现有 C++/MFC 外壳共存的方式。相比一次性重写，这允许逐个替换业务模块并保留现有皮肤、配置和插件行为；相比先维护一套独立 Rust 应用，这能让第一步迁移直接进入现有程序的运行路径。

阶段一提取 `CTrafficMonitorDlg::DoMonitorAcquisition` 中的网络计数差值与每秒速率计算。Rust 核心不依赖 Windows、MFC 或第三方 crate，通过按值传递整数和固定布局结构的 C ABI 被 C++ 调用。输出为独立 DLL，避免 Rust 与 MFC Debug/Release CRT 设置相互影响；跨边界不传递字符串、容器、指针或需要释放的对象。

## 阶段一行为契约

- 当前或前次计数的接收、发送字节均为零时，不计入本次流量。
- 切换连接，或者任一方向的计数回退（包括原 32 位计数器回绕）时，两方向均归零，保持现有行为。
- 正常情况下，增量为当前值减前值，速率为 `增量 * 1000 / 实际间隔毫秒`，按整数向下取整。
- 非正时间间隔保留合法字节增量，但速率归零，避免除零及负时间转换为异常速率。
- 乘除使用 128 位中间结果，最终超出 64 位的速率饱和为 `UINT64_MAX`。
- 首次采样、基线更新、连接选择、采集线程和历史流量累计仍由现有 C++ 调用方负责。

## 文件和构建边界

- 根目录 Cargo workspace 管理 `rust/traffic-monitor-core`。
- Rust 库同时输出 `rlib`（供 Rust 测试和后续模块使用）与 `cdylib`（供现有程序使用）。
- `include/TrafficMonitorCore.h` 声明 C ABI；`build/TrafficMonitorRust.targets` 在 C++ 编译/链接前执行 Cargo，并在链接后复制 DLL 到 EXE 目录。
- Visual Studio 的 x64、Win32、ARM64EC 分别映射到对应 MSVC Rust target；不将 ARM64EC 错当成普通 ARM64。
- 测试程序复用与正式工程相同的 MSBuild targets，并通过导入库真实链接、加载 DLL，验证结构布局、64 位计数和调用结果。
- 网速计算核心无第三方依赖；随后并行的 Windows 采集和 Shell 原型使用锁定的官方 windows-sys 0.61.2。Cargo.lock 纳入版本控制，先获取缓存，构建使用 `--locked --offline`。

## 验收与当前限制

必须通过 Rust 的业务边界测试、格式检查、Clippy，以及 C++ ABI 测试程序的 Debug/Release 构建和执行。正式 `TrafficMonitor.vcxproj` 必须导入同一构建规则，并在采集函数中实际调用 Rust。

本机初始 `TrafficMonitor_Lite.sln` 的 x64 Release (lite) 构建曾报 `MSB8041`（缺少 MFC）。2026-10-27 按用户要求补装 v143 MFC 后，完整 Lite 解决方案的 Release (lite) x64/x86 均已编译、链接通过，Rust DLL 分发与导入入口也已核对。ABI、完整程序编译和真实 GUI 运行仍是不同证据层级；当前尚未进行 MFC GUI 运行验收及 ARM64EC 完整构建。

## 后续迁移顺序

1. 网络计算核心与 C ABI 接入（本阶段）。
2. Windows 网卡枚举与 64 位计数采集：对照现有单连接/全部连接、连接重建与休眠恢复行为。
3. Rust Win32 任务栏原型：验证目标 Windows 版本、多屏 DPI、自动隐藏及 Explorer 重启恢复，再决定界面迁移细节。
4. 悬浮窗、托盘与设置逐步迁移，验证皮肤和配置兼容。
5. 历史流量持久化及插件边界迁移；旧 C++ 虚函数插件通过 C++ 桥接继续工作。

后续阶段各自定义验收并单独实施，不把本阶段的测试视为整项目重构完成。
