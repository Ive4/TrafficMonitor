# 重构发现

- 当前是 C++20 / MFC / MSVC v143 项目；主程序包含约百个编译单元，另有 C++/CLI 硬件桥和插件演示。
- 网络计数由 GetIfTable 读取 32 位字节字段；网络计算核心已迁 Rust，后续采集改用 GetIfTable2 的 64 位计数，按接口身份管理基线。
- 任务栏显示依赖 Explorer 窗口层级和 SetParent，Win10/Win11 有不同实现；需要真实宿主探测、多屏 DPI、自动隐藏及 Explorer 重启验证。
- 插件使用 C++ 虚函数 ABI，不能仅把 DLL 入口换成 Rust 就声称兼容；继续保留 C++ 桥接边界。
- 历史文件首行为 `lines: "N"`，记录为 `YYYY/MM/DD upload/download` 或旧版 mixed 总量，单位为 KiB；必须先验证兼容读写与合并语义。
- README 的现行方向是 Lite 与硬件监控插件；核心不主动重新引入内置温度驱动。
- Cargo 本地缓存已有 windows-sys 0.61.2 与 windows-link 0.2.1，可锁定依赖进行离线构建。
- 已实测：核心 17 个业务测试通过；x64/Win32 的 Debug/Release C++ 程序均真实链接并执行 Rust DLL，四组各 11 项 ABI 检查通过。
- 用户已明确重构版最低 Windows10，因此不保留 Win7/8 的 Rust 回退分支。
- 源码覆盖台账确认 3 个原 vcxproj 共 105 个编译单元，全部映射到 27 个功能域；新增编译单元未分类时检查脚本明确失败。
- 历史 mixed 数据只知道总量，同日合并不能推断为真实下载；原 C++ 的 mixed 序列化可能丢后来 split 上传量，Rust 保留未知方向并保证总量守恒。
- 旧 C++ 历史加载使用 signed atoll，Rust 完整 u64 round-trip 与旧加载器兼容导出是两个不同保证。
- ClientToScreen 对 RTL client 的两角可能反转；任务栏快照统一规范成物理 bounding Rect，增加纯回归用例。
- Cargo 可能在 manifest 改动或导入库未变时保留输出时间戳；自定义构建成功后刷新 DLL/导入库的验证时间可避免 MSBuild 永久判过期。
- 2026-10-27 用户要求安装 MFC：现有 VS 2022 Build Tools 17.14.37710.0 / MSVC 14.44.35207 已补装 Microsoft.VisualStudio.Component.VC.ATLMFC 与 Microsoft.VisualStudio.Component.VC.MFC.ARM64 及依赖；无需重启。完整 Lite Release x64/x86 解决方案构建成功，原 MSB8041 已解除。

## 当前交付结论

默认Cargo入口、发布CI与Bin/Rust双架构产物已切为独立Rust桌面程序；旧C++/MFC保留作兼容参照。完整迁移首轮254测试/插件集成为历史；设置与任务栏改造后两架构各265默认测试、6显式UI测试通过，Updated双架构产物与实际截图见docs/rust-migration/settings-tabs-acceptance.json。真实设备矩阵与当前温度提供者缺失等外部边界仍沿用总迁移文档，不能把ARM64EC类型检查说成ARM设备运行。

- 设置原实现每32字段分页、默认任务栏mask=3、宽度至少180、空选择当全选，是本次三个直接原因。现为7原生Tab、fresh mask=15/已有选择不覆盖、内容宽度自动适配、空选择隐藏。
- 单批ApplyPreferences在临时INI中完成typed/schema/皮肤/语言校验后提交，修复部分保存和A→B→A baseline问题；插件命中按实际rect缓存、完整混排用rust_taskbar/item_order兼容扩展保存。
- 自转储测试与并发PDH/COM载入可能互锁；实际minidump断言移至同测试程序的隔离子进程并加20秒上限，生产转储逻辑未变，两架构全套已重新通过。

- 计划任务实际XML导入发现此前字符串round-trip未覆盖的编码问题：UTF-8声明/无BOM字节交给本机schtasks报0x8004131a，CP936原始错误为“(1,40)::错误: 无法切换编码”；同内容UTF-16声明/LE BOM导入成功，生产生成器已统一为后者。
- schtasks中文stderr使用系统OEM代码页，不能用from_utf8_lossy；优先识别UTF-16/有效UTF-8，其余用Windows CP_OEMCP解码。真实任务回归只创建唯一命名的禁用任务，验证中文/实体路径并删除；没有执行任务动作。当前工具会话具有提升权限，未据此声称标准用户/真实登录已验收。
