# Rust Network Core Implementation Plan

> **For agentic workers:** Execute this plan task-by-task using the executing-plans workflow. Steps use checkbox syntax for tracking.

**Goal:** 将现有程序的网络差值与速率计算迁移到 Rust，并通过真实 C++ 调用验证 ABI 和构建集成。

**Architecture:** Rust 无依赖核心输出 rlib/cdylib。MFC 调用方通过 C ABI 获取增量和速率，继续管理采样基线和历史累计。C++ 测试程序复用正式工程的 MSBuild targets。

**Tech Stack:** Rust 2024 / MSRV 1.85、Cargo、C++20、MSVC v143、MSBuild。

## Global Constraints

- 工作分支：`refactor/rust-migration`。
- 本网络计算核心无第三方依赖；并行 Windows/Shell 扩展使用锁定官方绑定。Cargo.lock 纳入版本控制，构建使用 `--locked --offline`。
- 不迁移连接选择、线程、皮肤、配置、历史文件或插件接口。
- ABI 只使用固定宽度整数和 repr(C) 输出结构，不跨边界共享所有权。
- 原 MFC 基线缺少构建组件，完整应用的验证状态必须与 ABI 测试分开报告。

### Task 1: Rust 网络计算核心

**Files:** `Cargo.toml`, `Cargo.lock`, `rust/traffic-monitor-core/Cargo.toml`, `rust/traffic-monitor-core/src/{lib,network}.rs`, `rust/traffic-monitor-core/tests/network_samples.rs`。

**Interfaces:**

```rust
pub fn calculate_network_sample(
    received: u64, sent: u64, previous_received: u64, previous_sent: u64,
    elapsed_ms: i64, connection_changed: bool,
) -> NetworkSample;
```

- [x] 编写首次采样、连接切换、两方向回退、零计数、整数取整、非正间隔、64 位计数和溢出测试。
- [x] 先以返回零值的骨架运行 `cargo test --workspace --locked --offline -j 1`，确认正常采样与边界用例确实失败。
- [x] 实现设计中的规则，以 `u128` 中间值计算并限制最终速率。
- [x] 重跑测试，确认全部通过。

### Task 2: C ABI 与 MSBuild 集成

**Files:** `rust/traffic-monitor-core/src/ffi.rs`, `include/TrafficMonitorCore.h`, `build/TrafficMonitorRust.targets`, `tests/rust-core-ffi/{main.cpp,RustCoreFfi.vcxproj}`, `scripts/verify-rust-core.ps1`。

**Interfaces:**

```c
TmNetworkSample tm_calculate_network_sample(
    uint64_t received, uint64_t sent,
    uint64_t previous_received, uint64_t previous_sent,
    int64_t elapsed_ms, uint32_t connection_changed);
```

返回结构顺序为 `received_delta`, `sent_delta`, `receive_bytes_per_second`, `send_bytes_per_second`，均为 `uint64_t`。

- [x] C++ 使用静态断言检查结构大小/字段偏移，并以真实 DLL 调用检查正常、重置、时间和溢出结果。
- [x] MSBuild 按平台及 Debug/Release 映射 Cargo 参数，在链接前生成导入库并在链接后复制 DLL。
- [x] 运行验证脚本，要求 fmt、Clippy、Rust 测试、Debug/Release C++ ABI 测试均成功。

### Task 3: 接入现有监控链并记录结果

**Files:** `TrafficMonitor/TrafficMonitorDlg.cpp`, `TrafficMonitor/TrafficMonitor.vcxproj`, `.gitignore`, `docs/RUST_MIGRATION.md`。

- [x] 在 `DoMonitorAcquisition` 中用 Rust 输出替换原差值和除法代码，保留基线更新及历史累计。
- [x] 正式 vcxproj 导入与测试程序相同的 targets，并登记 ABI 头文件。
- [x] 忽略根目录 Cargo target；写明构建命令、DLL 分发要求、阶段状态与下一阶段验收。
- [x] 重跑所有 Rust/ABI 检查及完整 Lite 构建，准确记录 MFC 构建限制。
- [x] 检查 `git -c core.whitespace=cr-at-eol diff --check`、分支和文件清单；保留可审阅的工作区变更，不自动推送。
