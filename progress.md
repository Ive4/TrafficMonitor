# 执行记录

## 2026-10-27

- 从干净的 master 创建 `refactor/rust-migration`，没有提交或推送。
- 建立 Cargo workspace、网络计算核心、C ABI 头及正式 MFC 调用接入。
- TDD 红灯：17 个用例中 10 个在空实现上失败；实施后 17/17 通过。
- fmt、Clippy 通过；x64 和 Win32 的 Debug/Release ABI 各 11 项检查通过。
- 原 MFC 完整构建因缺少 MFC 组件报 MSB8041，基线与改造后验证分别记录。
- 用户追加要求：先完整盘点、再多个 agent 并发开发。采用 planning-with-files-zh 和 dispatching-parallel-agents，主 agent 负责共享文件与集成。
- 全量清单已覆盖 27 个改造域，台账与脚本覆盖原有 3 工程/105 编译单元；覆盖检查通过。
- 三个开发 agent 并行交付 Windows 采集、历史流量兼容核心、任务栏探测/布局/隐藏生命周期；每个独占 crate，未共享写入文件。
- 主 agent 完成 Rust CLI 参数 TDD（5 项先红后绿），集成逐接口网速、CPU/内存采样、历史样例校验和任务栏只读探测。
- 独立审查修复：ARM64EC CI target 未安装、CI 无缓存时缺少 cargo fetch、Win32 检查未传 Rust target、RTL client_screen_rect 反转。各问题已闭合，其他重要问题未发现。
- 最终 x64/i686 默认工作区测试各 94 项（含1 doctest）通过，0失败，2交互测试默认忽略；fmt、Clippy all-targets -D warnings 两架构通过。日志位于忽略目录 target/verify-x64.log 和 target/verify-win32.log。
- 四组 C++ ABI（x64/Win32 × Debug/Release）各11项通过；ARM64EC workspace/all-targets cargo check通过，仅类型/编译检查，未完成链接及ARM设备运行。
- x64 workspace Release构建通过；最新Release CLI实际采样、校验4条history fixture、探测1个主任务栏成功；剩余探针进程0。
- 主 agent 另在x64和i686分别显式运行2项交互宿主测试均通过，含8轮隐藏创建/验证/销毁与线程DPI恢复；当前只有Windows11/DPI96主屏实机证据。
- 原MFC完整Lite构建复验仍为MSB8041，尚未安装组件，未声称完整桌面应用构建或集成通过。
- MSBuild增量修正：scripts/build-rust-core.ps1在Cargo成功后刷新自有产物验证时间；连续两次Release x64构建第二次不执行Rust CustomBuild。
- 当前工作区变更未提交/推送；远端CI未触发。其余改造域和子功能保留在总清单，未将本批基础模块交付视为完整重写。

## MFC 安装与完整 Lite 构建复验

- 用户明确要求安装 MFC，使用现有 VS 2022 Build Tools 实例进行增量安装，未创建其他 VS 实例。
- 安装组件：`Microsoft.VisualStudio.Component.VC.ATLMFC`、`Microsoft.VisualStudio.Component.VC.MFC.ARM64` 及对应 ATL 依赖。安装退出码 0，vswhere 报 isComplete=true、isRebootRequired=false，VS 版本仍为 17.14.37710.0。
- MSVC 14.44.35207 的 `atlmfc/include/afxwin.h` 与 x64/x86/ARM64 的 `mfc140u.lib` 均已检查存在；组件注册状态同时通过 vswhere requires 检查。
- 完整 `TrafficMonitor_Lite.sln` 的 `Release (lite)` x86 构建退出码 0（56 个现有编译警告、0 错误），x64 完整解决方案构建退出码 0（主程序先完整编译通过，随后完整解决方案增量构建 0 警告、0 错误，含 PluginDemo）。原 MSB8041 阻塞已解除。
- 产物：`Bin/x64/Release (lite)/TrafficMonitor.exe`、`Bin/Release (lite)/TrafficMonitor.exe`，两目录均包含对应 `traffic_monitor_core.dll`，与各架构 Cargo 产物 SHA-256 一致。x64 EXE 导入表含 `traffic_monitor_core.dll`、`tm_calculate_network_sample` 与 `mfc140u.dll`。
- 安装结果保存在忽略目录 `target/mfc-install-result.json`；构建日志为 `target/mfc-build-x64.log`、`target/mfc-build-x64-solution.log`、`target/mfc-build-x86.log`。未启动 MFC GUI，未将编译成功视为运行验收；ARM64EC 完整构建另行验证。

## 连续执行完整 Rust 桌面迁移

- 用户再次明确完整改造，执行契约为 docs/rust-migration/full-rewrite-contract.md，目标独立Rust主EXE、不依赖MFC；不以单批库交付为本轮终点。
- 配置/皮肤/语言、桌面UI、插件ABI三路并发。配置域已交付21项x64/x86兼容测试、129项schema、18语言/15皮肤与原子IO；高级采集域继续完成CPU PDH/频率、GPU/磁盘和完整网络详情，两架构windows crate各33测试通过。
- 插件域完成生产Rust MSVC vtable和两架构C++ PluginDemo/Rust API1..8 fixture兼容验证，接续系统主题/自启动/更新服务；桌面域已实测独立窗口/托盘/任务栏及业务动作，主程序接入中。
- 主agent完成Selector自动/全选/手动、重连/回退/过滤器策略6项红绿测试；HistoryStore原子保存/恢复/损坏保留/未来记录隔离/跨日/导出7项红绿测试，native lib共13项通过。
- 配置agent接续独占native settings.rs/stats.rs映射与聚合；主agent专写app.rs/main.rs/打包和集成，不并发改同一文件。

## 独立主程序集成与当前验证（持续推进）

- `traffic-monitor-native` 已从空入口完成实际 Runtime 和 Win32 桌面启动。真实网速/CPU/内存连续采样成功，Selector、历史事务、配置重载、插件回调和业务动作已接入；新增native 20单元+3参数+settings/stats20用例通过（仍在增加）。
- 独立x64 Release EXE打包到 `Bin/Rust/x64/TrafficMonitor.exe`，Windows SDK资源编译带原图标、1.86.0版本、Win10兼容/DPI/Comctl32v6 manifest；约1.39MB，无需MFC主程序。新增Rust发布/验证/实际窗口抓图脚本，CI默认改Rust x64/x86，ARM64EC单独编译检查。PE导入和最终全矩阵待末轮确认。
- 实际EXE在独立en-US配置目录运行，浮窗真实皮肤+实时数据、六业务页、任务栏child创建和正常退出成功，截图 `target/native-desktop-captures/x64-20261027-043028-148/`。发现主窗额外图表线、历史列错位、taskbar PrintWindow纯黑等，已交给UI修复/证明真实可见像素，未据此宣称完整验收。
- 插件MonitorItem与DisplayItem从index10分岔已显式修复：Host10 GHz，11今日上传bytes，12今日下载bytes；mixed方向未知返回不可用。增加回归，未把旧总量伪装下载。
- 主题Apps/System双模式已遵原SystemUsesLightTheme；四主题预设、六托盘图标、皮肤mask/PNGalpha、主/任务栏独立样式/双击、历史Quarter+u128未知方向、localized schema/stats均接续实现。
- 纯Rust传感器WMI LHM/OHM+StorageReliability+可选System32 NVML完成，后台采样/取消有界退出，当前机器无provider四温度None；root已接mask/核心/磁盘选择和四类提醒。硬件环境缺读数不能声称真机温度值验收。
- 系统服务实际只读主题/启动查询、GitHub1.86在线更新成功；Gitee超时明确记录。插件版本XML检查确定性各架构6过，GitHub在线只读检查通过。未写真实用户启动项。
- root新增DirectWrite/Direct2D彩色字体文件，真实DIB测试证明同一emoji灰度/彩色开关与背景保留；空locale指针导致E_INVALIDARG已由实际失败定位并修复为空字符串。UI agent接入D2D开关/GDI回退。
- 当前三开发agent仍分域收尾；计划释放slot后启动独立等价行为、UI/ABI、持久化/生命周期审查并修复。任何未生效设置仍作为缺项，全部修改未提交推送。

## 本轮完整 Rust 主程序收口结果

- Cargo默认member已改traffic-monitor-native；独立正式EXE与Rust默认CI/资源打包均完成。旧C++/MFC源码保留兼容参照，不参与新程序构建，工作区未提交/推送。
- 最终统一脚本x64/x86各254测试通过（含1doctest），0失败，9个需要桌面/可信fixture/网络的用例默认忽略；fmt与workspace/all-targets Clippy -Dwarnings两架构通过。当前代码另有显式owned UI三项、Rust API1..8 fixture、旧C++ PluginDemo经过真实Rust后端的两架构集成通过。
- x64/x86 Release均实际连续采样并保存config/history/bak；最终独立EXE实际抓取六功能页、浮窗和可读任务栏文字，退出后验证进程0。ARM64EC workspace/all-targets check通过，仅编译检查，无ARM链接/设备运行声称。
- 最后补修坏历史文件的今日流量显示：原本错误显示0.00KB，定向红测后改为不可用--，原文件仍保留；正常PDH首样本warmup只写debug日志，不作为用户异常弹通知。
- Taskbar真实不可见问题已定位：Win11非layered child会被Explorer XAML合成覆盖，即使收到WM_PAINT；现在包括alpha255都使用layered，正式EXE与UI fixture均嵌Win8/10 manifest。当前屏幕截图已肉眼确认上传/下载文字。
- 产物Bin/Rust/x64/TrafficMonitor.exe：1627136bytes，SHA256=889F2E7B94D837931774228AB4ACAEE4280A1CF06C725BE1DB918A9570619F74。
- 产物Bin/Rust/x86/TrafficMonitor.exe：1473024bytes，SHA256=6DF800E98A6B6689DE94568B19CFB47B94CB69218BC42953C7AA7E3E053CF07D。
- 两PE导入检查确认无MFC、traffic_monitor_core.dll或OpenHardwareMonitorApi强制依赖；可选外部原生插件可有各自运行时需求。
- 打包含15套皮肤、18语言文件、原图标/版本/manifest/LICENSE，以及18个Cargo依赖和当前Rust标准库版权文本。脚本已实际重新打包验证，EXE hash未变。
- 机器可读交付记录为docs/rust-migration/acceptance.json；最终日志target/final-release-{x64,x86}.log、final-native-cpp-{x64,x86}.log、final-plugin-fixtures.log、final-arm64ec-check.log、final-imports-{x64,x86}.log。全部target证据为本地忽略文件，远端CI未触发。
- 外部验收仍需真实Win10/ARM、多屏混合DPI、真实Explorer重启/自动隐藏组合、登录自启动、可用传感器提供者、长期休眠/设备变化及任意第三方插件。当前四类温度无提供者真实None；这些与代码接入完成、当前Win11实测分别记录，不伪称全部设备已验。

## 设置 Tab 与任务栏体验修正（2026-10-27）

- 已按用户反馈完成7原生Tab，移除32字段分页；显示项目支持13内置/插件勾选和混合排序，推荐四项仅改任务栏草稿。切Tab保留编辑，Cancel/Escape丢弃，ApplyPreferences统一验证后提交，成功才更新baseline。真实字体/颜色/列表modal、失败后重试、A→B→A回归通过。
- 新配置任务栏默认上传/下载/CPU/内存；已有mask=3保留，通过“显示项目→推荐项目→应用”启用四项。默认自动适应内容，旧180下限自动迁移，自定义宽度保留手动选项，数值测量稳定宽度；取消全部不再当作全选，也不再留子窗口占位。
- 独立收口审查确认原4个P2全部闭环：部分提交、插件点击回落、插件混排序丢失、重复应用baseline错误。宽度读写/校验统一1..4096。生产minidump不变，实际dump测试移至有界独立子进程避免挂住并发PDH/COM测试。
- 统一脚本target/tabs-final-{x64,x86}.log：各265默认测试（含1doctest）通过，0失败，12环境用例默认忽略；两架构fmt、workspace/all-targets Clippy -Dwarnings、Release和3次真实采样/历史落盘通过。
- 两架构另显式执行全部6个desktop-ui环境测试，设置Tab/Apply/Cancel/语言回退、生命周期、注销取消、任务栏重建/Explorer几何恢复通过，日志target/tabs-all-ui-{x86_64,i686}-pc-windows-msvc.log。修正任务栏fixture为显式选中test，符合现在空选择隐藏语义；补跑两架构UI Clippy。
- Updated Release实际抓取中英文7个Tab、业务窗口和任务栏；x64实际2项/4项/13项/0项检查通过。4项任务栏可读CPU/内存，不只网速；0项无child；截图/尺寸与可执行文件hash见docs/rust-migration/settings-tabs-acceptance.json。
- 本次构建未覆盖旧TrafficMonitor.exe、未关闭用户实例、未修改日常配置或系统启动项，另存Bin/Rust/{x64,x86}/TrafficMonitor-Updated.exe，配套资源/README已更新；旧EXE hash仍与前里程碑一致。当前无遗留测试进程，工作区未提交/推送，远端CI未触发。
- ARM64EC workspace/all-targets check重新通过，仅编译；其余外部设备验收边界沿用总迁移文档。

## 重新打包（2026-10-27）

- 按用户要求重新构建x64/x86 Release，输出标准文件名Bin/Rust/{x64,x86}/TrafficMonitor.exe，生成TrafficMonitor-Rust-1.86.0-{x64,x86}-20261027-151607.zip。
- 每包151文件，含15套皮肤、18语言、原资源与18依赖许可证；清洁暂存目录不包含个人配置/历史、测试插件或调试PDB。ZIP解压后逐文件SHA256一致，解压出的EXE各完成3次真实采样并保存config/history/backup，正常退出。
- 本次只重建/打包与上述验证，未重跑先前完整测试。新包路径、hash与日志见docs/rust-migration/package-acceptance.json；代码未提交/推送。

## 搬动目录后自启动失败修复（2026-10-27）

- 用户截图报existing startup value points to another executable。只读确认当时的运行进程运行<install-dir>/TrafficMonitor.exe，HKCU Run/TrafficMonitor却指向Bin/Rust/x64/TrafficMonitor.exe；这是旧路径严格匹配导致的迁移失败，属于TrafficMonitor自身问题，与Orca路径显示issue不同。
- 新增有界Win32版本资源读取，按ProductName/OriginalFilename/CompanyName识别原C++和Rust TrafficMonitor，显式修改自启动时允许迁移其旧路径；只看文件名不足以认领。外部程序、缺失/未知文件和带参数命令仍保留；注册表写前重读快照，任务迁移覆盖前再确认原任务未变化。查询当前EXE是否已启用仍按实际路径，查询不会迁移启动项。
- 隔离注册表回归先红后绿，x64/x86均通过：旧/新路径含空格，重命名Updated、不同PE内容仍可迁移；伪装为同文件名的cmd.exe不能被覆盖/删除，取消自启动可移除已确认旧记录。测试只写Software/TrafficMonitorRustTests独占键，不创建真实登录任务。
- 统一脚本两架构各266默认测试、0失败、13环境用例默认忽略；fmt、workspace/all-targets Clippy -Dwarnings、Release、连续采样及历史落盘通过；另显式执行迁移fixture。ARM64EC全workspace/all-targets check通过，仅编译。
- 测试完成后对用户当前HKCU Run/TrafficMonitor进行一次明确的实际修复：先备份原值/两文件hash/进程到target/startup-repair/TrafficMonitor-Run-20261027-160355-638.json，确认值仍与诊断快照一致，再只更新该值到<install-dir>/TrafficMonitor.exe并重读核验。保持当时的运行进程运行，未替换安装目录中的运行EXE，未修改其他启动项/真实计划任务；真实注销登录启动尚未验证。
- 修复包TrafficMonitor-Rust-1.86.0-{x64,x86}-20261027-160446.zip，每包151文件，解压全文件SHA256比对及解压EXE隔离采样通过。实际执行的旧安装EXE仍为上一构建，新版修复在新包中；无需退出当前程序即可重试原自启动操作。最新记录docs/rust-migration/startup-migration-acceptance.json与package-acceptance.json，工作区未提交/推送。

## 计划任务 XML 编码错误修复结果

- 用户新截图0x8004131a及乱码，当时的安装实例为上一修复E8261EAA构建。生成XML声明/文件为UTF-8，无BOM，经实际schtasks私有禁用fixture复现“无法切换编码”；改为UTF-16 LE+BOM且声明一致，同内容实际导入成功。首轮仅字符串XML测试未覆盖系统导入的边界，现已补回归。
- 诊断原始bytes与结果target/task-scheduler-diagnosis/；额外验证不存在的私有嵌套任务目录可随导入创建，并清理任务及空目录。测试没有执行动作，未写真实用户自启动任务；实际修复另行记录。
- 两项新测试先同时失败，再x64/x86同时通过：真实生成器文件→schtasks导入→query_task回读中文/实体路径→删除；OEM中文错误保真且保留HRESULT。日志target/task-scheduler-red.log及task-scheduler-green-{x64,x86}.log。
- 统一验证初次被Clippy items_after_test_module拦下，已将测试模块移至platform末尾后重新执行全部验证，未放宽lint。

- 完整验证target/scheduler-final-{x64,x86}.log两架构各267默认测试、0失败、14环境用例默认忽略，fmt、workspace/all-targets Clippy -Dwarnings、Release和真实采样/配置历史落盘通过；额外显式执行两项计划任务回归，两架构均2/2通过。ARM64EC全workspace/all-targets check通过，仅编译。
- 实际修复首次按完整账号名核验时发现Windows任务系统将账号规范化为短用户名，拒绝后已回滚任务并保留原Run值。独立禁用fixture确认将该短名解析为SID与当前用户完全一致，改用SID核验后再修复成功；此为一次性修复脚本核验调整，程序运行路径仍由current_exe动态获取。
- 当前真实任务`\TrafficMonitor\Autorun for <current-user>`已成功创建/回读：Enabled=true，执行<install-dir>/TrafficMonitor.exe，当前账号InteractiveToken、HighestAvailable、当前用户LogonTrigger、Delay=PT3S。先保存target/task-scheduler-repair/20261027-170726-695/before.json，再确认任务身份/动作/触发器，移除同一EXE的重复HKCU Run值；after.json记录完整结果。保留原运行实例与安装EXE，未手动执行任务、未做注销登录；创建证据来自当前提升权限会话，不代表标准用户已实测。
- 每包151文件的新ZIP为TrafficMonitor-Rust-1.86.0-{x64,x86}-20261027-170926.zip，已解压逐文件SHA256核验并从解压EXE完成隔离真实采样。最新记录docs/rust-migration/task-scheduler-acceptance.json和package-acceptance.json，代码未提交/推送。
