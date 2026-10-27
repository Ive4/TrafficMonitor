# 设置 Tab 与任务栏内容宽度 Implementation Plan

> **For agentic workers:** Use subagent-driven-development / executing-plans within the existing authorized branch.

**Goal:** 使设置按功能Tab可操作，任务栏指标可勾选且只占实际内容空间。

**Architecture:** UI agent独占windows.rs/settings_tabs.rs/localization.rs；主agent独占layout.rs/model.rs、native设置映射、默认schema、脚本/文档。所有保存从draft统一提交，复用DesktopOptions/Dashboard。

**Tech Stack:** Rust、Win32 Tabs/ListView、GDI/DirectWrite、现有Cargo workspace。

## 任务

- [x] UI：写真实Tab/跨Tab保留/Cancel回归并观察红灯；实现分类Tab和统一draft；真实验证颜色/字体/列表和Apply/Cancel。
- [x] 显示选择：13内置+插件复选列表、上下移动、空选择保持；先添加空选择与默认选择失败回归。
- [x] 宽度：提取 taskbar_panel_width(needed,fixed_minimum,available)，先复现180下限问题；自动宽度、数字位数和固定选项映射、DPI/双行/插件实际测量。
- [x] 集成：消费新的auto_width/value_digits字段，测量和创建统一宽度策略；无可见项销毁child并恢复Explorer空间。
- [x] 验收：x64/x86相关测试、Clippy、Release，使用隔离实例抓真实Tab和2/4/更多指标布局；独立review后交付新包。保留现有用户实例与配置。

证据与产物：`docs/rust-migration/settings-tabs-acceptance.json`。x64/x86各265默认测试、6显式UI测试通过；当前Win11/DPI96截图通过，设备矩阵边界沿用总迁移文档。
