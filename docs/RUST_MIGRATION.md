# TrafficMonitor Rust 重构交付与验收

分支 `refactor/rust-migration`，基线 `930f175`。新默认入口是独立 Rust `TrafficMonitor.exe`，支持 Windows 10/11。主程序、桌面窗口、采集、持久化、配置、插件宿主和系统服务均由 Rust 实现；旧 C++/MFC 源码保留作兼容参照，默认构建与发布已切换到 Cargo，不再经过旧主程序或 Rust 桥接 DLL。

## 构建和运行

需要 Rust/MSVC 工具链及 Windows SDK；新程序不需要安装 MFC 开发组件。首次执行 `cargo fetch --locked`，之后可离线构建：

```powershell
rustup target add x86_64-pc-windows-msvc i686-pc-windows-msvc
./scripts/build-rust-release.ps1 -Architecture x64
./scripts/build-rust-release.ps1 -Architecture x86
./Bin/Rust/x64/TrafficMonitor.exe
```

验证/抓图脚本使用 PowerShell 7。如果 PATH 中的 Cargo 不可用，可向脚本传 `-Cargo "$env:USERPROFILE/.cargo/bin/cargo.exe"`。输出目录分别为 `Bin/Rust/x64/` 和 `Bin/Rust/x86/`，包括 EXE、原图标/图片、15 套皮肤、18 个语言文件和许可证。不要只复制 EXE 而丢掉资源目录。

```powershell
./scripts/verify-rust-desktop.ps1 -Architecture x64
./scripts/verify-rust-desktop.ps1 -Architecture x86
# 在有 Explorer 的交互桌面显式运行；默认 CI 不动任务栏。
./scripts/verify-rust-desktop.ps1 -Architecture x64 -DesktopSmoke
./scripts/capture-rust-desktop.ps1 -Architecture x64
```

实际采样和限时 GUI 验证必须指定隔离数据目录，避免覆盖日常配置：

```powershell
./Bin/Rust/x64/TrafficMonitor.exe --data-dir target/my-smoke --headless --samples 3 --no-plugins
./Bin/Rust/x64/TrafficMonitor.exe --data-dir target/my-smoke --exit-after-ms 4000 --taskbar --no-plugins
```

## 27 个功能域的实现与接入

源码映射 `rust-migration/source-inventory.json` 覆盖旧 3 工程的全部 105 个编译单元；`scripts/update-migration-inventory.ps1 -Check` 检查覆盖。下列“已接入”说明生产入口调用了对应实现，实际验证边界另列，不能据此推断未提供设备的系统版本已经实测。

| ID | 功能域 | Rust 实现及当前边界 |
|---|---|---|
| RM-01 | 工程/构建/发布 | Cargo 9 crates、锁文件、资源/版本/manifest、x64/x86发布脚本和Rust默认CI已接入；旧MSBuild仅兼容参照 |
| RM-02 | 网速计算 | core 与 native/selection 的64位增量、首样本、回退、时间和溢出处理已接入 |
| RM-03 | 网卡选择/详情 | GetIfTable2、IP地址/DNS/网关/MAC/MTU、稳定LUID、自动/手动/全选、隐藏/过滤接口、断连重连已接入 |
| RM-04 | CPU/内存/频率 | 系统时间与PDH选择、物理内存、ProcessorInformation频率；独立失败不会丢掉网络计量 |
| RM-05 | GPU/磁盘 | PDH实例枚举、GPU引擎聚合、磁盘实例选择与异常诊断已接入 |
| RM-06 | 硬件温度 | 纯Rust后台WMI LHM/OHM、StorageReliability和可选System32 NVML；使能位、CPU核心/磁盘选择及缺值诊断已接入。提供者缺失时为None，不能用ACPI热区假装CPU温度 |
| RM-07 | 历史存储 | 旧KiB格式、mixed未知方向、跨日、原子保存、持续更新备份、损坏原件和未来记录归档已接入；导入保留进程内不足KiB余数 |
| RM-08 | 历史查看 | 真正列表列、日历、日/周/月/季度/年、范围过滤、u128聚合、线性/对数图、导入/导出已接入 |
| RM-09 | 配置 | 旧键/编码/注释、130项schema（含补齐的启动方法）、批量验证、原子保存、APPDATA/便携/Temp保护及显式数据目录已接入 |
| RM-10 | 皮肤 | INI/XML、预览/大小布局、插件ID、紧凑颜色顺序、独立覆盖、BMP mask/PNG alpha、主题皮肤选择已接入 |
| RM-11 | 绘图 | GDI/GDI+及可选DirectWrite/Direct2D文字、彩色emoji开关、透明、DPI、资源RAII和失败回退已接入；不逐字移植旧D3D/MFC包装 |
| RM-12 | 浮窗 | 真实数据、拖动、置顶、锁定、穿透、透明、边缘/越界、双击、菜单、全屏策略已接入 |
| RM-13 | 任务栏 | Explorer宿主、实际layered子窗、任务按钮保留/恢复、DPI、自动隐藏和TaskbarCreated重建路径已接入；真实多屏/Win10设备矩阵仍待外部验收 |
| RM-14 | 任务栏样式 | 独立字体/颜色/显示项/顺序、插件动态宽度/双行、主题预设、透明/偏移/留白、网络与资源图不同量程和开关已接入 |
| RM-15 | 托盘/实例 | 原6种图标、主题适配、提示/菜单/恢复、Explorer重建与按配置目录单实例已接入 |
| RM-16 | 设置界面 | 7个原生Tab、跨Tab草稿、整批校验/应用/取消、字体/颜色/列表及显示项目复选/排序已接入；由皮肤定义的尺寸/背景/项目只读 |
| RM-17 | 插件宿主 | Rust MSVC vtable/thiscall、API1..8、发现/禁用/启用、配置/命令、字体/DPI/语言/数值回调、自绘颜色与输入/宽度/卸载已接入；旧DLL是外部可信原生代码 |
| RM-18 | 显示值 | 13个内置项目、插件、标签/位字节/单位/精度、主窗与任务栏独立格式、图表量程已接入 |
| RM-19 | 生命周期/启动 | 定时采样、显式checkpoint、注销取消继续运行、最终退出、注册表/任务计划显式应用、实际状态查询、Restart Manager参数保留已接入 |
| RM-20 | 语言 | 原语言包/系统选择、English回退、参数替换、旧text/menu键、设置/菜单/统计及插件语言同步已接入；系统日历部分仍受系统区域设置影响 |
| RM-21 | 更新 | HTTPS主程序及插件XML清单、严格版本/结构/大小/超时/固定来源检查、后台通知已接入；仅检查，不执行下载 |
| RM-22 | 诊断 | profile error.log、panic hook、异常minidump、重复/并发保护、设备和文件错误通知、后台退出诊断已接入；进程内转储为尽力收集，不是原生插件崩溃沙箱 |
| RM-23 | 原MFC控件行为 | Win32原生列表/组合框/字体/颜色/日历/滚动/焦点和独立Rust窗口已替换，无MFC主程序依赖 |
| RM-24 | 系统工具 | 主题/强调色、路径/编码/原子IO、动态DPI后加API、Shell工具及受控子进程已接入，Win7/8分支退休 |
| RM-25 | 资源/样例 | 原资源与LICENSE打包、原图标/版本资源、Rust桌面和插件兼容样例；旧PluginDemo作跨语言验证fixture |
| RM-26 | 验收 | x64/x86自动检查和当前Windows11桌面验收持续收口；Win10、多屏/不同DPI设备、ARM设备、真实登录自启动与长时间运行明确单列外部验收 |
| RM-27 | 默认入口切换 | 独立Rust正式EXE与Rust默认CI/打包已切换；旧C++/MFC代码保留作参照，不参与新主程序构建 |

旧 `.NET未安装提示` 配置键继续保留兼容读取，但新宿主不加载旧C++/CLI桥，不在界面提供已经失去意义的.NET安装选项。`last_light_mode`是内部状态，不作为可编辑选项。130个旧/补齐schema字段不等于130个必须暴露给用户的控件。

## 独立审查与修复

两路独立审查分别检查等价行为/数据与UI/ABI/发布。已修复并有定向回归的关键问题包括：

- 历史导入丢失KiB余数、备份停在第一次保存、mixed总量不应推断下载。
- 隐藏接口仍参与统计、quoted连接列表保存错误、采样失败后分母只算一次间隔及CPU/内存失败阻断网络历史。
- 插件Host MonitorItem和DisplayItem索引10以后的差异：Host10为GHz，11/12为今日上传/下载bytes；分别格式化，不使用总网速/今日总量替代。
- 未发送插件自绘EI0/1文字颜色、只更新主题深浅而遗漏强调色、文本插件未接输入、动态插件宽度与双行。
- 注销/关机缺checkpoint、跨DPI拖动重放旧坐标、皮肤/语言/连接模式/历史范围未正确回显。
- 无效皮肤编辑项、自启动操作不可达及未知状态不应自动发送禁用、网速/CPU图表开关与量程混用、历史图DPI/滚动坐标。
- Win11非layered子窗被Explorer合成覆盖：实际child现在即使alpha255也保持layered，并给正式EXE和显式UI fixture声明Win8/10兼容。

## 验证证据与外部边界

完整主程序首轮收口日志和产物证据见 `progress.md` 与 `rust-migration/acceptance.json`；该里程碑的x64/x86各254项默认测试及插件集成为历史记录。后续设置与任务栏改造的当前结果见 `rust-migration/settings-tabs-acceptance.json`：两架构各265项默认测试、6项显式UI测试、fmt、Clippy、Release和真实采样通过。ARM64EC仅完成全工作区编译检查。旧第一批94测试和MFC构建记录为历史。

已取得的实际证据：当前Windows11上真实网络/CPU/内存/GPU/磁盘/频率采样；独立Release EXE读取/保存隔离配置及历史；原皮肤浮窗、清晰任务栏上传/下载文字、6个功能页和正常退出。抓图位于忽略目录 `target/native-desktop-captures/`，只抓自有进程窗口及自有任务栏child矩形，不把PrintWindow返回成功当作可见文字成功。

原C++ PluginDemo与独立Rust API1..8 fixture已在x64/x86显式验证；配置窗口的真实用户交互、第三方硬件提供者、任意外部插件不因此自动获得验收。传感器在本机缺少LHM/OHM/NVML时真实返回None；未声称获得了CPU/主板温度。GitHub主程序/插件版本检查真实成功，Gitee超时有诊断。自启动默认测试只写独占临时注册表fixture，没有修改用户TrafficMonitor启动项。

外部验收仍需要Windows10/ARM实机、多屏混合DPI、不同任务栏/自动隐藏策略、真实登录和长时间休眠/设备变化场景。ARM64EC编译检查与实际链接/设备运行必须分开表述。

## 设置 Tab 与任务栏改造（2026-10-27）

设置改为7个真实Tab：常规、悬浮窗、任务栏、显示项目、提醒、高级、皮肤与语言。取消32字段分页；切换Tab保留未提交内容，应用统一校验所有Tab后一次提交，取消/Escape丢弃草稿。字体、颜色、列表编辑器保持原生控件；不改动自启动选项时不写系统启动项。

显示项目Tab可独立勾选和排序13项内置指标及插件，完整混合顺序可保存重读。新配置默认上传、下载、CPU、内存；已有只选网速的配置仍保留原选择，可点任务栏列表下的“推荐项目”后应用，恢复这四项。悬浮窗项目若由皮肤定义，界面明确标注只读。

任务栏默认自动适应内容宽度，旧默认180宽迁移为自动，自定义宽度保留为手动最小值。只为选中内容留位，使用稳定数字测量减小抖动；全不选则移除任务栏子窗口。当前Win11/DPI96实际抓图验证2项、4项、13项及空选择，x64/x86显式UI验证任务栏重建/几何恢复与设置应用/取消；未扩大真实Win10、多屏和混合DPI的验收声称。

本次可执行文件另存为 `Bin/Rust/x64/TrafficMonitor-Updated.exe`、`Bin/Rust/x86/TrafficMonitor-Updated.exe`，避免覆盖当时运行中的旧EXE。关闭旧实例后启动对应Updated文件；程序沿用同目录/既有配置规则。测试全部使用独立数据目录，不修改日常配置。当前工作区仍未提交/推送。

## 后续自启动路径修复与最新包

程序搬到安装目录后，旧HKCU Run值仍指向构建目录，旧版严格比较EXE路径会拒绝启用。现在显式应用自启动设置时可识别并迁移原C++/Rust TrafficMonitor的旧记录；版本资源核对产品、原文件名及作者，不仅比较文件名，其他程序仍受保护。注册表/任务重读检查防止覆盖已经变化的记录；状态查询保持只读。

上一轮路径迁移修复使用标准文件名`TrafficMonitor.exe`，fixture、当时实际Run启动项修复和验证边界见`rust-migration/startup-migration-acceptance.json`。该轮x64/x86各266默认测试与显式迁移回归通过，仅更正已确认的旧Run路径并备份，没有关闭/替换运行实例。

后续又修复了真实计划任务导入中的0x8004131a：XML声明与临时文件统一为UTF-16 LE+BOM，中文命令输出按UTF-16/UTF-8/OEM代码页正确解码。x64/x86当前各267默认测试、2项显式真实系统导入/中文错误回归通过；导入回归创建唯一禁用任务、回读中文/实体路径后删除，不执行动作。当前用户请求的计划任务已另行备份/安装并按SID、EXE动作、登录触发器验证，移除匹配的重复Run项，保留旧进程/旧安装EXE；真实登录启动与标准用户权限下创建仍未验证。最新版交付和准确边界以`rust-migration/task-scheduler-acceptance.json`及`package-acceptance.json`为准。
