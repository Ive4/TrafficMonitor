# TrafficMonitor Rust 插件宿主

本 crate 是独立 Rust 主程序的原生插件服务，直接实现 `include/PluginInterface.h` 的 MSVC 单继承虚表 ABI。生产实现没有 C++ 垫片、MFC import 或旧主程序运行依赖。GDI/GDI+ 来自 Windows 系统库；第三方插件自身仍可以依赖 MFC。

## 接入

```rust,ignore
let mut plugins = PluginManager::new(HostInfo {
    config_dir: app_config.join("plugins"),
    ..HostInfo::default()
});
plugins.discover(&app_dir.join("plugins"), &disabled_file_names)?;
// 创建窗口后及显示状态改变时更新真实 HWND、DPI、主题、语言、值和资源表。
plugins.update_host(host_info)?;
plugins.refresh(monitor_info)?;
let items = plugins.items()?;
// rect 使用 x/y/width/height，不是 left/top/right/bottom。
plugins.draw(&item_id, hdc, [x, y, width, height], dark, taskbar)?;
for notification in plugins.take_notifications() { /* 宿主在回调结束后显示通知 */ }
plugins.shutdown()?;
```

`PluginInfo.id` 是 DLL 文件名（禁用匹配不区分大小写），`PluginItem.id` 是插件提供的全局唯一项目 ID。发现同时保留禁用项和加载错误项供设置界面显示。`load` 返回明确错误；`discover` 将每个 DLL 的加载失败保存在 `PluginInfo.error`，一个坏 DLL 不阻止发现其他 DLL。不存在的插件目录视为空集合。目录发现拒绝逃逸插件目录的符号链接；加载先解析绝对路径，使用 `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS`，不使用当前工作目录搜寻依赖。

`HostInfo.config_dir` 指向插件配置目录本身，宿主自动添加末尾反斜杠并创建目录。`strings` 键为 `(resource_key, section)`；`extended` 使用头文件 EI 索引 0..16，EI_CONFIG_DIR=17 自动发送。`update_host` 只发送变化的扩展设置，避免每次采样重新读取插件配置。绘制前单独发送 EI_DRAW_TASKBAR_WND。主程序须提供真实值和格式化文本；本层不猜测主程序的单位/标签规则。

公共入口包括发现、加载、启禁、HostInfo 更新、监控数据和数据请求、显示项/资源图值/双行标志、真实 HDC 测宽、tooltip、插件图标、设置对话框返回值、菜单命令/图标/勾选状态、鼠标和键盘事件、自绘及关闭。选项对话框只应由用户动作调用。禁用文件名的持久化由主程序配置层负责。

## ABI 和回调

| 插件 API | 本层允许调用的新增接口 |
| --- | --- |
| 1 | 基础元信息、项目、DataRequired、设置、OnMonitorInfo、旧 DrawItem |
| 2 | tooltip、扩展信息、插件图标 |
| 3 | HDC 测宽、鼠标 |
| 4 | 键盘；OnItemInfo 是空枚举预留协议，无已定义宿主动作 |
| 5 | 命令数量/名称/图标/执行/勾选 |
| 6 | 资源图开关/值 |
| 7 | OnInitialize 与 ITrafficMonitor 指针 |
| 8 | DrawItemEx 与双行独占 |

拒绝版本小于 1 或大于 8，避免未知接口布局。`TMPluginGetInstance` 是 C 导出；x86 虚成员用 `extern "thiscall"`（this 在 ECX），x64 用统一 Windows `system` ABI。所有输入输出结构使用明确大小；`MonitorInfo` 为 56 字节，最后的 cpu_freq 偏移为 48。接口没有虚析构函数槽位。

ITrafficMonitor 的 12 个方法完整实现：接口版本 1、应用版本、13 类数值与两个窗口格式化文本、通知、语言、配置目录、两类 DPI、主题色、字符串资源、两个 HWND。通知先复制到队列，避免从 ABI 回调递归进入应用消息处理。

IPluginDrawer 的 10 个方法完整实现：接口版本 0、文本/对齐/多行/透明度、替换绘图裁剪区域、填充、边框/圆角/虚线、HBITMAP 的拉伸/裁剪填充/等比适应、图标、真实文字测量、线段、HDC。透明度通过内存 DC 混合实现。所有 GDI 对象有对应释放，传入 HBITMAP/HICON 仍属插件。插件遗留 DrawItem 先调用一次，再调用 API8 DrawItemEx，与旧宿主顺序一致；两种绘图可以同时贡献内容。外层 SaveDC/RestoreDC 保持宿主 DC 状态，绘图错误可返回给调用方。GDI+ 在 LoadLibrary 之前启动，并在 DLL 全部卸载之后关闭。

## 生命周期和边界

- PluginManager 是 `!Send + !Sync`，所有方法和回调必须在创建它的 UI 线程执行。错误线程的 Host 回调在访问可变状态前返回空值；关闭后的 Host 回调也不访问业务状态。宿主应在插件模态对话框期间避免重入自己的可变后台对象。此接口不支持插件异步跨线程回调。
- DLL 独占插件及显示项对象；本层只借用指针，绝不 delete、Box::from_raw 或调用不存在的析构接口。所有返回给 Rust UI 的字符串立即复制为自有 String。Host 返回的 UTF-16 指针可在当前同步调用中使用，在下一次 HostInfo 更新前有效；插件应复制需要长期保存的字符串。Drawer 指针只在本次 DrawItemEx 内有效。
- `plugin_icon` 和命令图标句柄是借用值，只在 DLL 加载期间有效。UI 如需跨禁用/关闭保存图标，必须自己复制图标。
- shutdown 先关闭 Host 回调，然后依次卸载 DLL，最后才释放 Host 和 GDI+。同线程同步调用已结束才允许关闭。旧协议没有 OnShutdown；自行创建后台线程的第三方插件必须自行保证卸载安全，这不能由宿主虚构保证。
- 插件是进程内可信原生代码。空对象/虚表/槽位、过长字符串、重复/空 ID、超过 1024 个显示项或命令、选项非法返回值有明确错误。无效指针、越界原生访问、SEH、跨边界 C++ 异常不能由 Rust catch_unwind 隔离，本 crate 不声称具有这种隔离能力。
- 暂不实现旧宿主对第三方 DLL/MFC 的 DrawText 导入表篡改。正常 GDI/GDI+ 与 API8 Drawer 在 Rust 宿主中直接绘制；依赖该私有 hook 修正的 DirectComposition 透明文字效果仍需具体插件实测。Win10、ARM64/ARM64EC 和第三方硬件插件的设备验收单独进行，不由本机 ABI 测试替代。

## 验证

默认测试只使用本 crate 的内存对象/临时无效 DLL，不扫描用户 plugins。显式 fixture 测试仅加载仓库原 PluginDemo 和本 crate 本地编译的 Rust DLL，不打开可见模态设置对话框。

```powershell
cargo test -p traffic-monitor-plugins --offline
cargo clippy -p traffic-monitor-plugins --all-targets --offline -- -D warnings
cargo test -p traffic-monitor-plugins --offline --target i686-pc-windows-msvc
cargo clippy -p traffic-monitor-plugins --all-targets --offline --target i686-pc-windows-msvc -- -D warnings
./rust/traffic-monitor-plugins/verify-fixtures.ps1
```

2026-10-27 本机验证：x64/x86 单元测试（12 项）、Clippy 和显式双 fixture 集成测试通过。测试覆盖 API1..8 门控、布局与参数、监控回调、主程序全部回调、通知复制、设置三种返回值及非法值、菜单勾选、输入消费、图形实际像素/测宽/裁剪、线程约束、禁用发现/重载、错误记录、重复 ID、空实例、版本越界，以及 `GetModuleHandleW` 验证禁用/退出后 DLL 真正卸载。原 PluginDemo 的 4 个项目和两个自绘项在 x64 与 x86 均执行成功。

真实 C++ fixture 路径：

- `Bin/x64/Release/plugins/PluginDemo.dll`
- `Bin/Release/plugins/PluginDemo.dll`

`fixtures/rust_plugin.rs` 由 rustc 单独编译为 cdylib，没有 Cargo workspace/C++ 依赖，展示原生 Rust 构造插件、项目虚表以及双向 ABI 调用。它是带参数断言的 ABI 验收样例，专供测试宿主，不应直接复制到用户 plugins。脚本输出留在 `target/plugin-fixtures/<target>/`。原 PluginDemo 的设置模态窗口没有自动点击；配置 ABI 的 Changed/Unchanged/NotProvided 路径由非模态 Rust 对象测试验证，用户交互设置和桌面整体整合由主程序验收负责。
# 绘制颜色上下文

`set_draw_colors(item_id, label, value)` 在绘制前向该显示项所属的 API >= 2 插件发送 EI_LABEL_TEXT_COLOR / EI_VALUE_TEXT_COLOR，参数为 COLORREF 的十进制字符串。此调用不广播其他扩展设置，也不改变 HostInfo；调用者必须在同一 UI 线程紧接着绘制该项。API 1 跳过新槽。x64/x86 Rust fixture 均在 DrawItem 内验证收到了正确颜色；单测覆盖版本边界、未知项和关闭状态。当前插件单测总计 13 项。
