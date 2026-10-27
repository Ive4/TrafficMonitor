# Windows 高级采集

`advanced.rs` 是独立 Rust 主程序的只读硬件和网络详情层。使用 `windows-sys` 调用原生 Windows API，不依赖 MFC、WMI 脚本或硬编码采集值。

## 公共接口

```rust
HardwareSampler::new() -> HardwareSampler
HardwareSampler::sample(&mut self, disk_name: &str) -> HardwareSnapshot
network_details() -> std::io::Result<Vec<AdapterDetail>>
```

`HardwareSnapshot` 保留 `cpu_usage_percent`、`cpu_frequency_mhz`、每逻辑处理器 `cpu_frequencies_mhz`、`gpu_usage_percent`、`disk_usage_percent`、全部 `(磁盘实例名, 使用率)` 与 `diagnostics`。百分比和代表频率为 `Option<f64>`；缺失、预热、驱动/权限错误返回 `None` 和明确诊断。只在 Windows API 真正报告零利用率时返回 `Some(0.0)`。

`HardwareSampler` 通过 `PhantomData<Rc<()>>` 明确限制在创建线程使用和销毁。所有成功打开的 PDH query 都由 RAII 在退出和构造失败路径中关闭。`new` 收集首个样本，`sample` 在至少 200 ms 后收集第二个样本；不会为了预热阻塞 UI 线程。调用者一般使用已有的 1 秒采样定时器。

## 实际采集

- CPU 利用率：英文 PDH `\Processor(_Total)\% Processor Time`。原配置的 `cpu_usage_acquire_method` 由运行时选择此值或原有 `GetSystemTimes` 采集；此层不把 PDH 错误替换成合成零值。
- CPU 频率：`GetActiveProcessorCount(ALL_PROCESSOR_GROUPS)` 决定缓冲区大小，`CallNtPowerInformation(ProcessorInformation)` 返回 `PROCESSOR_POWER_INFORMATION.CurrentMhz`。完整向量保留系统返回的各逻辑处理器值，代表频率取非零值均值。这是 Windows 电源信息所报告的节流后频率，不宣称为 CPU 厂商的瞬时 Turbo 遥测。
- GPU：英文 PDH `\GPU Engine(*)\Utilization Percentage`，按 `engtype_` 后的引擎类型合计实例，选择最高类型再限制为 0–100%。不同引擎类型不会直接相加。没有有效引擎返回 `None`。
- 磁盘：英文 PDH `\PhysicalDisk(*)\% Idle Time`；先将 Idle 限制到 0–100%，再计算 `100-idle`，避免 NVMe/RAID 多队列超过 100% 的 Idle 产生负使用率。支持配置中的完整实例名，找不到指定名时明确诊断并回退 `_Total`，同时暴露所有磁盘实例供设置 UI 选择。
- PDH 仅接受 `CStatus=0/1` 和有限浮点值。数组大小探测、实例动态变化重试、64 MiB 上限、条目数量与字符串指针边界检查均已实现；字符串复制后释放缓冲区。大小变化时重新以空缓冲区探测，不依赖不充足缓冲区调用返回的长度。

## 网卡详情

`GetAdaptersAddresses(AF_UNSPEC)` 返回真实 IP 接口，包含 gateway/prefix 标志，保留 `NET_LUID` 与 `GetIfTable2` 的稳定身份。每个 `AdapterDetail` 包括接口索引、别名、描述、AdapterName、MAC、IPv4/IPv6 地址和前缀、IPv6 scope、DNS、网关、MTU、链路速度、物理/运行状态。所有字段均复制为 Rust 所有权数据。

物理判断优先使用 `GetIfEntry2` 的 `HardwareInterface`，查询失败时才按接口类型 6/71 回退；明确排除 `FilterInterface` 标记的 WFP/QoS 等过滤层，不按中文/英文接口名称猜测，也不把物理接口 IP 复制给过滤层。真实隧道和 Hyper-V 接口可以有独立 IP，保留它们并标为非物理，由上层连接选择策略决定是否统计。

IP Helper 使用初始化且至少 8 字节对齐的缓冲区，处理 `ERROR_BUFFER_OVERFLOW` 并最多重试 5 次。结构长度、指针范围、链表循环和最多节点数均有检查。真机 x86 API 的部分链表节点只有 4 字节对齐，而 Rust SDK 内含 `u64` 的 union 要求 8 字节；因此有界后使用 `read_unaligned` 复制结构，避免形成未对齐引用。UTF-16 字符串另行检查 2 字节对齐和 NUL 边界。

## 验证证据

2026-10-27 当前 Windows 设备实际运行：

- x64 与 i686 `cargo test -p traffic-monitor-windows --all-targets --offline` 各 **33/33** 通过，其中高级采集新增 **13 项**（11 个单元测试、2 个集成测试）。包括 GPU 分组/无效状态/NaN、磁盘 Idle 和选择回退、native 指针/短结构/循环边界、IPv4/IPv6 前缀及 scope、FilterInterface 位、真实网卡 LUID 关联。
- 两架构 `cargo clippy -p traffic-monitor-windows --all-targets --offline -- -D warnings` 通过。
- 指定本域 3 个 Rust 文件的 `rustfmt --check --edition 2024` 通过。
- 两架构 `cargo run -p traffic-monitor-windows --example advanced --offline` 真实只读运行。首帧 CPU/GPU/磁盘均返回预热 `None`；随后 x64 CPU 约 1.8–4.6%、GPU 约 4.8%、磁盘约 0.1%；x86 后续 CPU 约 4.4–4.6%、GPU 约 21.6–21.8%、磁盘约 0.8%。两架构均读取 16 个逻辑处理器，频率向量为 12×2400 MHz、4×1800 MHz，均值 2250 MHz。不同时间的利用率差异是真实采样，并非两架构值相等的断言。
- 当前设备发现物理 Realtek 以太网以及隧道、Hyper-V 和 loopback IP 接口，IPv4/IPv6、DNS、网关、速度、MTU 与物理标记均有实际返回。

示例只读取硬件/网络数据，不更改系统、注册表、网卡或性能计数器配置。没有将当前设备通过推广为所有驱动/多 GPU/多处理器组/ARM64EC/Windows 10 设备验收；这些设备尚未实测。当前未安装 Rust 1.85 工具链，本域使用的标准库功能不晚于项目声明的 MSRV，但真实 1.85 编译仍由统一 CI 验证。

依据：[PDH formatted counter array](https://learn.microsoft.com/en-us/windows/win32/api/pdh/nf-pdh-pdhgetformattedcounterarrayw)、[GetAdaptersAddresses](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getadaptersaddresses)、[MIB_IF_ROW2 flags](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/ns-netioapi-mib_if_row2)、[PROCESSOR_POWER_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/power/processor-power-information-str)。
