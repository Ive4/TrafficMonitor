# TrafficMonitor Rust 重构执行计划

## 目标和约束

- 分支：`refactor/rust-migration`；起点：`930f175`。
- 用户确认重构版只支持 Windows 10/11，Rust 路径默认启用。
- 用户要求先整理全部改造内容，再由多个 agent 并行开发。
- 总清单：`docs/RUST_MIGRATION.md`；源码覆盖台账：`docs/rust-migration/source-inventory.json`。
- 共享 Cargo workspace、锁文件、C ABI、MFC 接入、CI 与最终验收由主 agent 维护。

## 执行阶段

1. [x] 新建分支、基线检查、Rust 网络计算核心与 C++ ABI 接入。
2. [x] 按现有功能和编译单元整理完整迁移范围、依赖及验收标准。
3. [x] 第一批并行开发：Windows 网络/CPU/内存采集、历史流量格式核心、Win32 任务栏适配原型。
4. [x] 主线集成：Rust 只读采样入口、统一构建/测试、源码清单覆盖检查、CI target 安装。
5. [x] 独立审查并记录实测证据、已完成范围和剩余阶段。

以上是第一批基础模块的历史阶段记录，当时全量重构尚未完成。本轮独立Rust主程序收口结果见下文及 docs/rust-migration/acceptance.json。

## 当前连续执行：完整独立 Rust 主程序

用户已明确要求使用多个 agent 并行完成完整改造。执行契约为 `docs/rust-migration/full-rewrite-contract.md`，不再以独立库或单批基础模块完成作为本轮终点。

- [x] 并发实现兼容配置/皮肤/语言、Win32 桌面 UI、旧插件 ABI。
- [x] 接入真实采集、选择/去重、硬件指标与网络详情。
- [x] 接入历史持久化、聚合、导入导出与图表。
- [x] 完成系统自启动/更新/诊断与资源打包。
- [x] 新正式 Rust EXE 替换旧主程序入口，PE 无 MFC 强制依赖。
- [x] RM-01..27 生产路径已接入，独立审查修复、x64/x86统一验证及当前Win11桌面验收完成；设备外部验收边界另列。

## 设置与任务栏体验修正

- [x] 7个原生Tab替换字段分页，跨Tab保留草稿，统一校验/应用/取消。
- [x] 独立主窗/任务栏显示项复选、完整混排保存、推荐上传/下载/CPU/内存。
- [x] 自动内容宽度替代旧180空白下限，保留自定义手动宽度，全不选移除子窗。
- [x] 独立审查4个P2闭环，两架构各265默认测试与6显式UI测试通过；另存Updated双架构包，记录实际截图及验收边界。

证据：`docs/rust-migration/settings-tabs-acceptance.json`；原主程序迁移里程碑记录继续保留。

## 计划任务 XML 与中文诊断修复

- [x] 复现schtasks 0x8004131a，按系统代码页恢复“无法切换编码”原始错误。
- [x] 实际对比UTF-8失败与UTF-16 BOM成功，补真实禁用任务导入/回读/清理及错误解码红绿回归。
- [x] 修复声明与落盘编码一致、UTF-16/UTF-8/OEM输出解码；x64/x86两项定向测试均通过。
- [x] 统一fmt/Clippy/全工作区测试/Release，ARM64EC编译，修复当前用户计划任务并备份旧启动配置。
- [x] 重打双架构ZIP，解压完整性/采样检查，记录验证边界并交付。

## 状态准则

`planned` 尚未实施；`in_progress` 正在开发；`implemented` 已写代码但尚未通过集成验收；`verified` 已有对应验证证据。独立模块通过不代表 MFC 应用或完整桌面重构完成。

## 错误与限制

- 语义检索连续返回 Transport closed，已按规则转为 rg 与精确源码读取；不重建索引。
- 原始 Lite x64 构建曾因缺少 MFC 报 MSB8041；2026-10-27 已补装，完整 Lite x64/x86 解决方案构建通过，此阻塞已解除。
- Rust 1.98 会将本机中文链接器“正在创建库”的提示报告为 linker_messages；代码 Clippy 已通过。
- MSBuild CustomBuild 曾因命令换行及 Cargo 保留未变产物时间戳反复执行，现为单行脚本入口；Cargo 成功后更新自有 DLL/导入库验证时间，第二次同配置构建已确认跳过。
- 现有 vcxproj 使用 CRLF，差异检查使用 `git -c core.whitespace=cr-at-eol diff --check`。
