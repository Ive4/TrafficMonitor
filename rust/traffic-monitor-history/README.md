# traffic-monitor-history

RM-07 的独立纯 Rust 历史流量核心，Rust 2024 / MSRV 1.85，无第三方依赖。仅处理内存字符串、字节和记录，不访问系统日期、不读写文件，因而不会覆盖现有用户历史。

## 接口

```rust
use traffic_monitor_history::{Date, HistoryDocument, MergeMode};

let today = Date::new(2026, 10, 27)?;
let mut history = HistoryDocument::parse(
    "lines: \"2\"\r\n2026/10/27 10/20\r\n2026/10/26 100\r\n",
)?;
let report = history.normalize(today)?;
assert!(report.discarded_future.is_empty());

let backup = HistoryDocument::parse("lines: \"1\"\n2026/10/27 15/5\n")?;
history.merge(&backup, today, MergeMode::RestoreBackup)?;
let totals = history.aggregate_range(Date::new(2026, 10, 1)?, today)?;
assert_eq!(totals.upload_kib(), 15);
assert_eq!(totals.download_kib(), 20);
assert_eq!(totals.mixed_kib(), 100);
assert_eq!(totals.total_kib(), 135);

history.rollover(Date::new(2026, 10, 28)?)?;
let old_program_text = history.to_legacy_compatible_string()?;
assert_eq!(HistoryDocument::parse(&old_program_text)?, history);
# Ok::<(), Box<dyn std::error::Error>>(())
```

- `Date::new(year, month, day)`：1900..=3000，完整公历月长度及世纪闰年校验。
- `HistoryRecord::mixed(date, total)` / `split(date, upload, download)`：KiB，非字节；每项及同日总和使用 checked `u64`。`mixed` 的 `download_kib()` 只是旧文件存储位置，表示未知方向的总量。
- `HistoryDocument::parse(&str)` / `parse_bytes(&[u8])`：严格 `Result`；支持 LF、CRLF、空行及首行 UTF-8 BOM。坏日期、非十进制/负数/溢出计数、坏行、坏头部、声明行数不符均返回诊断，含物理行号，不暴露部分成功文档。头部的 N 是非空数据行数，空行不计入。
- `normalize(today)`：今天在首行，历史日期降序，同日合并，补零今天，移除空的过去日期；未来记录通过 `NormalizationReport::discarded_future` 返回，调用方可展示或留存。错误不改变原文档。
- `merge(other, today, mode)`：普通合并 checked 相加；备份恢复今天取逐方向最大值、已存在的过去日期保留当前记录，其他日期补入。源文档不变，目的文档在溢出时也不变。
- `rollover(today)`：由调用者注入日期，保留上一日使用量，新日补零，同日重复调用不重复归档；时钟回退的未来数据同样在报告中返回。
- `aggregate_range(start, end)`：含首尾日期，不改变原始记录；`u128` 分别汇总已知上传、已知下载、未知方向总量。日期倒置报错。可作为日/月/年的范围统计基础；本模块不定义周编号或 UI 分组策略。

## 旧格式与有意修正

依据 [HistoryTrafficFile.cpp](../../TrafficMonitor/HistoryTrafficFile.cpp) 的 `WriteTrafficRecord`、`Load`、`Merge`、`MormalizeData` 及 [CommonData](../../TrafficMonitor/CommonData.h) 的字段和总量定义：

```text
lines: "2"
2026/10/27 4294967296/8589934592
2026/10/26 12884901888
```

旧行仅存总量，不能恢复上传/下载；新行存上传/下载。序列化使用相同首行、固定 `YYYY/MM/DD`、十进制 KiB 与 CRLF，无额外标签。原始解析后序列化保持记录次序、零值和 mixed 标志；换行及空白会规范化。

C++ 同日去重仅逐字段相加并保留首项 mixed 标志，mixed 在前时可能在再次写出时丢失后来 split 行的上传量；C++ 也会把今天的 mixed 强制改成 split。这里有意修正：**只要参与同日累计的任一记录为 mixed，结果仍是 mixed，总量 checked 相加**，不虚构方向、不丢总量，且与输入次序无关。备份恢复当天若任一记录 mixed，则取两个总量的较大值并保留 mixed；全 split 时仍逐方向取最大值。过去记录同时存在时，备份恢复保留当前记录。

归一化把每条有效旧日期记录统一处理，避免 C++ 特殊首行在后面又出现今天记录时可能丢掉旧首行。解析比 C++ 的 `atoi/atoll`、1..31 日校验和静默跳坏行更严格；错误必须由调用者处理后才能导出。未来记录仍按旧程序策略从规范化文档中移除，但会完整返回给调用者。

Rust 解析与 `to_legacy_string()` 支持完整 `u64` 并保证自身 round-trip。旧 C++ `Load` 使用有符号 `atoll`，所以给旧程序导出请用 **`to_legacy_compatible_string()`**：任何单字段超过 `i64::MAX` 都会明确拒绝。此检查只保证旧十进制文本加载范围；不保证旧 UI 后续 signed 字节换算对极端值安全。不得把 Rust 的完整 u64 round-trip 当成旧 C++ 对完整 u64 的验收。

## 验证与未接入范围

```powershell
& 'cargo' test -p traffic-monitor-history --locked --offline -j1
& 'cargo' fmt -p traffic-monitor-history -- --check
& 'cargo' clippy -p traffic-monitor-history --all-targets --locked --offline -j1 -- -D warnings
& 'cargo' run -p traffic-monitor-history --example history_compat --locked --offline -j1
```

测试先观察失败再实现，覆盖旧/新样例、32 位以上与 u64 边界、坏行诊断、真实日历、同日 mixed 次序无关/总量守恒、排序/幂等、备份策略、错误原子性、跨日/跨年/闰日/时钟回退、范围聚合及文本 round-trip。样例仅在 stdout 输出，不读写真实历史。

**现有 MFC 历史加载、保存、图表、日历和跨日链尚未接入此 Rust 库。** 当前兼容依据为原源码、检查入库的文本样例与 Rust 测试；未声称已运行旧 MFC 程序导入验收。RM-08 的列表/日历、周编号、月年视图、线性/对数渲染与导入导出 UI 仍待迁移；文件持久化、原子替换、备份恢复文件操作也由后续集成处理。
