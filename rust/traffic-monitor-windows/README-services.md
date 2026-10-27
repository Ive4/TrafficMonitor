# Windows 系统服务

services.rs 提供真实主题读取、两种自启动方式、用户触发的 Shell 打开动作和更新检查。

主题读取 HKCU Personalize 的 AppsUseLightTheme / SystemUsesLightTheme；键缺失使用 Windows 默认浅色，访问失败返回错误。DwmGetColorizationColor 转为 COLORREF 0xBBGGRR。

注册表自启动仅操作 HKCU Run 下的 TrafficMonitor 值，完整引用绝对 exe 路径。创建/删除前校验已有命令；同名不同路径返回冲突，不覆盖/删除。计划任务兼容 TrafficMonitor 产品文件夹和当前用户名；用户身份由 GetUserNameExW 获取。schTasks 使用系统目录绝对路径、分离参数、无 shell 拼接、隐藏窗口、空 stdin、20 秒上限和 1 MiB 输出上限。查询采用 XML/HRESULT，不依赖本地化错误文字。任务使用当前用户 InteractiveToken、HighestAvailable、登录延迟 3 秒、不因电池停止、无限执行时长。创建不使用覆盖参数；启用/删除前校验唯一 Exec 命令且无附加参数。

启用一种模式后，只移除另一模式中指向当前 exe 的条目。两种系统不能形成跨系统事务，第二步失败返回真实错误，第一步可能已生效；调用者应重新读取两种状态。构造、采样和只读探针不调用 set_auto_start。

Shell 路径先确认存在；应用必须是绝对 exe；URL 只接受不含空白/控制字符的 HTTP(S)。独立短生命周期 STA 调用 ShellExecuteExW，避免改变采样/UI线程的 COM apartment。UNC 和长路径前缀正确规范化。

更新 source=0 GitHub raw、source=1 Gitee raw，沿用 version_utf8.info。WinHTTP 保留 HTTPS 证书校验，DNS/connect/send/read 均有限超时，读取时检查总时间，最多 1 MiB，不追随重定向。quick-xml 拒绝 DTD、重复关键字段、截断文档、超过 32 层/4096 节点。版本按数值分量与 prerelease 比较。按当前架构选择链接；链接必须是 github.com/gitee.com 的本项目 releases 路径。说明按系统 UI 语言选择中/繁/英。只返回信息，绝不下载或执行更新。

验证命令：

```powershell
cargo test -p traffic-monitor-windows --offline services::
cargo test -p traffic-monitor-windows --offline --target i686-pc-windows-msvc services::
cargo clippy -p traffic-monitor-windows --all-targets --offline -- -D warnings
cargo run -p traffic-monitor-windows --offline --example services_probe
cargo run -p traffic-monitor-windows --offline --example services_probe -- --update=0
```

2026-10-27：13 项服务单测通过，x64/x86 编译与 Clippy 已验证。注册表测试只操作专用临时 HKCU 测试键，验证引号、冲突保护和清理，没有写真实 Run 或计划任务。只读探针读到应用/系统深色主题及真实 DWM 色值，产品计划任务不存在时返回 false。GitHub 在线检查成功解析 1.86 和对应发布链接；Gitee 返回 WinHTTP 超时 12002，错误如实保留。

真实自启动创建/删除、登录触发和应用打开属于用户操作验收，未自动修改真实用户配置进行验证。第一次负例测试发现 URL 校验前 trim 会剥离尾部换行并调用 ShellExecute（example.test）；已改为先校验原字符串并通过回归，未关闭用户浏览器进程。

依据仓库 auto_start_helper.cpp、TrafficMonitor.cpp、WindowsSettingHelper.cpp、UpdateHelper.cpp 和 Microsoft 官方文档：

- https://learn.microsoft.com/en-us/windows/win32/taskschd/schtasks
- https://learn.microsoft.com/en-us/windows/win32/api/winhttp/nf-winhttp-winhttpsettimeouts
- https://learn.microsoft.com/en-us/windows/win32/winhttp/option-flags
