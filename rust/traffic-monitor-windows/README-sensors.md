# 可选硬件温度服务

`SensorSampler` 提供真实 CPU/GPU/硬盘/主板温度与可选 CPU 核心、硬盘名称列表。所有无效、过期、不存在或被禁用的数据返回 `None`，不会把 ACPI thermal zone 当作 CPU 温度，也不会生成零温度占位值。

数据来源依次为：已运行并公开 WMI 的 LibreHardwareMonitor、OpenHardwareMonitor；硬盘缺失时查询 Windows Storage 的 `MSFT_StorageReliabilityCounter`；GPU 缺失时尝试 System32 中 NVIDIA 驱动自带的 NVML。不会启动外部监控程序、安装驱动或从工作目录搜索 DLL。没有这些提供者或硬件不支持时，诊断明确指出缺失来源。原 C++/CLI 硬件桥返回 STL/共享指针，不作为 Rust C ABI 加载。

WMI 根据 HardwareType 和父设备关系识别传感器，保留重名设备，CPU 可按名称或平均值选择，硬盘同设备的多个温度探头取平均。GPU 优先 NVIDIA、AMD、Intel，优先 GPU Core 温度。返回值必须有限且在 `(0, 200]` 摄氏度范围内；OpenHardwareMonitor 将空值发布为 0，故 0 视作无数据。

采样在专用 MTA 线程运行，接口与 DLL 句柄始终归该线程所有。UI `sample` 使用容量 1 的非阻塞请求/结果通道，通常每 2 秒请求一次，切换选择即时重算已有值，数据超过 10 秒失效。失败提供者缓存 30 秒。WMI 枚举每次 Next 最长 100 ms，每次查询枚举预算 2 秒、最多 4096 行；连接使用 Windows 的 MAX_WAIT 标志（操作系统上限约 120 秒），不会阻塞 UI 采样调用。

`shutdown()` 设置停止标记、断开请求、调用 CoCancelCall，等待工作线程 RAII 释放 COM/NVML 后 join。正常退出测试确认先清理后返回。等待超过 3 秒返回明确错误；第三方 COM 提供者或 NVML 内部调用不能被 Rust 安全强杀，若其忽略取消，资源继续由未结束的线程持有，Drop 最终可能分离该线程直到调用返回。不能将这一边界描述为绝对的 3 秒原生调用终止保证。

验证（2026-10-27）：x64/x86 均通过 10 项传感器单测，包括平均/名称选择、父设备分类、无效值、重复名称、禁用、缓存过期、选择切换与退出释放。只读实机探针未发现可用温度提供者，四类返回 None；x64 首次结果约 100 ms、采样调用 10 us、退出 10 ms，x86 首次结果 301 ms、采样调用 11 us、缓存调用 28 us、退出 10 ms。未验证有真实传感器硬件的温度准确性，不能把合成测试或缺失提供者的探针当作四类硬件实测。

```powershell
cargo test -p traffic-monitor-windows --offline sensors:: --lib
cargo test -p traffic-monitor-windows --offline --target i686-pc-windows-msvc sensors:: --lib
cargo run -p traffic-monitor-windows --offline --example sensors
```

参考：

- https://learn.microsoft.com/en-us/windows-hardware/drivers/storage/msft-storagereliabilitycounter
- https://docs.nvidia.com/deploy/nvml-api/latest/api/group__nvmlDeviceQueries.html
- https://raw.githubusercontent.com/openhardwaremonitor/openhardwaremonitor/master/WMI/Sensor.cs
- https://raw.githubusercontent.com/LibreHardwareMonitor/LibreHardwareMonitor/master/LibreHardwareMonitor.Windows.Forms/TestScripts/basicwmi.py
