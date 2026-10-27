# 独立 Rust 桌面程序完整迁移执行契约

用户于本轮明确要求多个 agent 并发完成完整改造。沿用 `refactor/rust-migration` 与 Windows 10/11 边界，连续实施，不将独立库、原型或混合程序构建成功视为完整交付。

## 交付目标

新的正式入口为独立 Rust `TrafficMonitor.exe`，构建和运行不依赖 MFC/C++ 主程序。旧源码保留为兼容行为和测试参照。原有图标、语言、皮肤等资源复用；旧插件作为外部二进制兼容，不要求用户重编译已有插件。

27 个改造域的功能均应有明确实现和接入路径。实现、自动测试、当前桌面实测、Win10/多屏/ARM设备等外部验收分别记录。不得把仅保存但未执行的设置、只读探针、禁用按钮或硬编码零值登记为功能完成。

## 本轮并行写入边界

1. 配置/皮肤/语言 agent：独占 `rust/traffic-monitor-config/`，读取 CIniHelper、SettingsHelper、CommonData、StrTable、SkinFile 和现有皮肤/语言。实现兼容 INI 编码与键、原子保存、配置路径、语言回退、INI/XML 皮肤和显示格式。
2. 桌面 agent：独占 `rust/traffic-monitor-desktop-ui/`，实现 Win32 消息循环、浮窗、托盘、实际任务栏显示、菜单/设置/历史/网络/插件窗口、绘图及恢复。通过该 crate 的 `DesktopBackend` trait 消费运行时，不直接修改公共 Cargo 或数据文件。
3. 插件 agent：独占 `rust/traffic-monitor-plugins/`，实现 MSVC 插件接口兼容加载、API 版本、项目/配置/命令/自绘/主程序回调和独立 Rust 插件样例。若必须使用 C++ 兼容垫片，应先报告必要性，不引入 MFC 主程序依赖。
4. 主 agent：独占根 workspace/lock、`rust/traffic-monitor-native/`、已有采集/历史库的接入和补全、资源/打包/CI/验收脚本、公共文档；负责协调 API。

后续空闲 slot 继续处理系统服务、GPU/磁盘/频率、网络详情、更新、诊断及验收补缺。禁止清理共享 target、还原别人的变更、提交/推送或重新建分支；公共文件只能由主 agent 写入。

## 公共接口约束

### 配置 crate

配置 agent 尽早冻结 API，至少提供 IniDocument 的 `parse`/`parse_bytes`、`get`、`get_string`、`get_bool`、`get_i64`、`set`、`set_bool`、`set_i64`、遍历、保留未知键及原子保存。路径、语言、皮肤与格式化使用独立模块，向主 agent 报告类型后再接入。错误不得静默覆盖真实配置；文件测试使用临时目录。

### 桌面 crate

桌面 agent 自有 `model.rs` 并尽早冻结。目标接口：

```rust
pub trait DesktopBackend {
    fn options(&self) -> DesktopOptions;
    fn sample(&mut self) -> Result<Dashboard, String>;
    fn dispatch(&mut self, action: DesktopAction) -> Result<(), String>;
    fn attach_windows(&mut self, main: usize, taskbar: Option<usize>);
    fn text(&self, key: &str, fallback: &str) -> String;
    fn draw_plugin(&mut self, id: &str, hdc: usize, rect: [i32; 4], dark: bool, taskbar: bool) -> bool;
    fn checkpoint(&mut self) -> Result<(), String>;
    fn shutdown(&mut self) -> Result<(), String>;
}
pub fn run(backend: Box<dyn DesktopBackend>) -> Result<(), String>;
```

DesktopOptions 描述浮窗/任务栏/托盘、字体颜色透明度、布局、显示项、主题、坐标和采样间隔等。Dashboard 含真实显示项目、图表值、网络选项/详情、历史记录/范围、插件信息、提示与设置字段。DesktopAction 覆盖设置应用/保存、连接选择、历史范围/导入导出、皮肤语言、插件配置/命令、自动运行和更新等。agent 可精简命名，但必须尽早发出完整数据结构，禁止塞入只读按钮替代动作。

窗口代码必须考虑 Win32 重入和线程亲和；不在可重入调用间保留重叠可变借用。托盘与 Explorer 重建、资源销毁必须有真实控制路径；任务按钮几何若临时调整，退出和失败时恢复。CI 默认测试不操作用户任务栏或设置。

### 插件 crate

插件 agent 参考 `include/PluginInterface.h`，尽早冻结 PluginManager API，至少提供发现/加载/禁用、数据刷新、显示项目、配置/命令、绘图、HostInfo 更新和安全卸载。所有 MSVC vtable/thiscall/system 细节要有依据、边界检查和测试；不跨边界错误释放插件对象。生产代码为 Rust，原 PluginDemo 可作现有 C++ 插件兼容 fixture。

## 集成与验收

- 新 Rust 可执行程序实际读取采集、保存配置/历史、显示浮窗/任务栏/托盘，设置和菜单修改后生效，退出释放资源。
- 未识别/损坏历史保留原文件并诊断，恢复/导入导出不静默丢数。mixed 总量不推断为真实下载。
- 网卡身份区分物理/过滤器，自动/手动/全选策略可控，避免默认重复统计。
- 新程序启动不自动改变用户自启动项；仅应用明确的设置操作时写注册表/任务计划。
- 更新只检查与展示；不自动执行下载内容。
- x64/x86 fmt/Clippy/test/Release/真实桌面验收；ARM64EC 编译与设备运行分开。最终检查 PE 导入，确认新主程序没有 MFC 或旧 Rust 桥接 DLL 的强制依赖。
- 逐项回填 RM-01..27；任何缺项保持未完成并继续开发，不以“第一批完成”结束本轮完整改造。
