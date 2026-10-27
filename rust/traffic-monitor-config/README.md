# traffic-monitor-config

独立 Rust 主程序的配置、资源查找、语言、皮肤与显示格式库。对照 `IniHelper`、`SettingsHelper`、`TrafficMonitor::LoadConfig`、`StrTable`、`SkinFile`、`SkinManager`、`DisplayItem` 和 `Common` 的实际源码实现；没有调用 MFC 或旧桥接 DLL。

## 接口与行为

- `IniDocument::{parse,parse_bytes,load,get,get_string,get_bool,get_i64,set,set_bool,set_i64,entries,sections,remove_section}`。UTF-8 BOM、无 BOM UTF-8、UTF-16 LE/BE、Windows ACP ANSI；显式损坏 BOM/UTF-16 返回错误。无 BOM 首选有效 UTF-8，再用系统 ACP 解码。保留未修改的注释、未知键、未知行与重复项；重复键首项读取/修改，后续项保留。保存统一 UTF-8 BOM/CRLF。
- 引号、旧 `$value$` 包裹、换行/回车/制表符/引号/反斜杠转义、双引号、逗号字符串列表、整型数组尾项延伸与布尔位数组。Windows 盘符、UNC、相对路径中的 `\t`/`\n` 保持为路径字符。`FontInfo` 使用原程序的四位整数 `font_style`，顺序为粗体、斜体、下划线、删除线。
- `IniDocument::save_atomic(path)` 和公共 `save_bytes_atomic(path, bytes)`：同目录唯一 `create_new` 临时文件，写入、flush、sync，再调用 Windows `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`。失败保留原文件并清理本次临时文件。公共字节接口不加 BOM、不创建目录或备份；历史文件可复用。
- `AppPaths::{resolve,discover,ensure_config_dir,migrate_from,plugin_config_dir,resource}`：读取模块旁 `global_cfg.ini`，遵守便携模式；缺少全局配置时根据 `%APPDATA%/TrafficMonitor/config.ini` 是否存在选择；目录不可写时回退 AppData。迁移为显式操作，仅复制缺失文件，保留源文件。路径字段包括配置、历史、日志、语言、皮肤和插件目录；不会改注册表或启动项。
- `settings_schema()` 提供 129 个有标签、类型、默认值、范围和枚举选项的设置字段，保留 `histroy_traffic`、`alow_out_of_border` 等原拼写。`settings_schema_with_resources` 加入语言/皮肤候选值；`SettingSpec::{value,validate}`、`apply_setting` 支持设置 UI，`normalize_settings` 可显式生成缺省值并报告非法数值。动态显示标签保持缺省缺失，供选定语言计算，防止空标签覆盖默认文本。
- `LanguageCatalog::load(dir)` 枚举外部语言包，内嵌中英文三个默认包；`select("")`/`select("0")` 跟随 Windows 系统区域语言，其余支持 BCP-47 和旧 `BCP-47|名称|译者` 格式。同代码不同名称的语言包独立保留。`Language::text` 按节/键回退到内嵌英文，`format` 和 `format_parameters` 单次解析 `<%1%>` 等占位符，不重新解释参数内容。`LanguageInfo` 包含译者、网址、字体、资源路径、配置串和 Windows language ID，供插件同步。
- `Skin::{load,from_ini,from_xml,layout,scaled_layout,apply_overrides,save_overrides}`：支持旧 INI 与 XML；XML 优先，解析错误返回诊断。布局保留全部内置/插件节点、插件 ID、对齐、可见性、坐标、宽高、标签、字体、颜色与小/大预览位置。`SkinCatalog::select` 根据所选皮肤和系统深浅色配置选择并加载独立 `skin_名称` 覆盖项。不存在的明确选择返回错误。
- `enumerate_skins` 枚举有皮肤文件的目录。背景优先成对 PNG，回退 BMP；保留可选 mask 文件路径。`Skin::{background,mask}`/`ImageResource::load` 实际读取文件，验证 PNG/BMP 文件头和尺寸并返回原字节；像素解码、PNG 透明合成与绘制由桌面渲染层负责。
- `FormatOptions::from_ini` 读取悬浮窗/任务栏原配置键，支持自动/固定 KB/MB、简洁模式、精度覆盖、字节/比特、空格/单位/百分号、使用率/温度/频率和内存已用/可用格式。`DISPLAY_ITEMS` 保留 13 个原项目 ID、INI 键、索引和语言键，`display_item_label` 与 `display_order` 提供默认标签和排序。

## 验证

当前 Windows 环境真实执行 21 项集成回归，x64 与 i686 各 21/21 通过：全部 18 个仓库语言包、15 个皮肤目录（含 `xml_test` 插件布局）、真实 PNG/BMP 读取、UTF-16/ACP、四位字体组合、未知项/重复项、旧路径与列表、主题覆盖、格式边界、设置范围和标签空格、二进制原子写入、目标被独占锁定时原字节保留及临时文件清理。测试写入仅使用各用例临时目录。

```powershell
cargo test -p traffic-monitor-config --all-targets --offline
cargo test -p traffic-monitor-config --all-targets --target i686-pc-windows-msvc --offline
cargo clippy -p traffic-monitor-config --all-targets --offline -- -D warnings
cargo clippy -p traffic-monitor-config --all-targets --target i686-pc-windows-msvc --offline -- -D warnings
cargo fmt -p traffic-monitor-config --check
```

以上测试和 Clippy/fmt 均通过。当前仅安装较新的 stable MSVC 工具链；代码使用的标准库接口均不晚于声明的 Rust 1.85，但没有在真实 1.85 工具链上复跑。当前桌面绘图、系统主题变化事件、打包后的资源位置与完整主程序设置生效需由主程序/桌面集成验收；本库测试不等同于这些验收。未进行 ARM64EC 设备或 Windows 10 设备运行验证。
