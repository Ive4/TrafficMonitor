param([switch]$Check)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$projects = @(
    'TrafficMonitor/TrafficMonitor.vcxproj',
    'OpenHardwareMonitorApi/OpenHardwareMonitorApi.vcxproj',
    'PluginDemo/PluginDemo.vcxproj'
)

function Get-MigrationTasks([string]$Project, [string]$Name) {
    if ($Project.StartsWith('OpenHardwareMonitorApi/')) { return @('RM-06') }
    if ($Project.StartsWith('PluginDemo/')) { return @('RM-17') }
    switch -Regex ($Name) {
        '^TrafficMonitorDlg$' { return @('RM-02', 'RM-12', 'RM-15', 'RM-19') }
        '^(AdapterCommon|SelectConnectionsDlg|NetworkInfoDlg)$' { return @('RM-03') }
        '^(CPUUsage|CpuFreq)$' { return @('RM-04') }
        '^(GpuUsage|DiskUsage|PdhQuery)$' { return @('RM-05') }
        '^HistoryTrafficFile$' { return @('RM-07') }
        '^(HistoryTraffic.*|CalendarHelper)$' { return @('RM-08') }
        '^(IniHelper|SettingsHelper)$' { return @('RM-09') }
        '^CommonData$' { return @('RM-07', 'RM-09', 'RM-18') }
        '^(SkinFile|SkinManager|SimpleXML|TinyXml2Helper|tinyxml2|SkinDlg|CSkinPreviewView)$' { return @('RM-10') }
        '^(Draw.*|TaskBarDlgDrawCommon|IDrawCommon|D2D1Support|D3D10Support1|DCompositionSupport|Dxgi1Support2|Image2DEffect|WIC|SupportedRenderEnums|RenderAPISupport|HResultException|CVariant)$' { return @('RM-11') }
        '^(TaskBarDlg|ClassicalTaskbarDlg|Win11TaskbarDlg|WineTaskbarDlg|TaskbarHelper)$' { return @('RM-13') }
        '^(TaskbarDefaultStyle|TaskbarItemOrderHelper|TaskbarColorDlg|TaskBarSettingsDlg|Win11TaskbarSettingDlg)$' { return @('RM-14') }
        '^(IconSelectDlg|AppAlreadyRuningDlg)$' { return @('RM-15') }
        '^(OptionsDlg|GeneralSettingsDlg|MainWndSettingsDlg|MainWndColorDlg|CAutoAdaptSettingsDlg|SkinAutoAdaptSettingDlg|DisplayTextSettingDlg|SetItemOrderDlg)$' { return @('RM-16') }
        '^(PluginManager|PluginManagerDlg|PluginInfoDlg)$' { return @('RM-17') }
        '^DisplayItem$' { return @('RM-18') }
        '^(TrafficMonitor|auto_start_helper)$' { return @('RM-19') }
        '^StrTable$' { return @('RM-20') }
        '^AboutDlg$' { return @('RM-25') }
        '^(UpdateHelper|PluginUpdateHelper)$' { return @('RM-21') }
        '^(crashtool|MessageDlg)$' { return @('RM-22') }
        '^(BaseDialog|CMFCColorDialogEx|ColorSettingListCtrl|ColorStatic|ComboBox2|CTabCtrlEx|FileDialogEx|LinkStatic|ListCtrlEx|PictureStatic|SpinEdit|StaticEx|TabDlg)$' { return @('RM-23') }
        '^(Common|FilePathHelper|DllFunctions|WindowsSettingHelper|WindowsWebExperienceDetector|WinVersionHelper)$' { return @('RM-24') }
        '^stdafx$' { return @('RM-01') }
        '^Test$' { return @('RM-26') }
        default { return @('UNCLASSIFIED') }
    }
}

$units = foreach ($project in $projects) {
    $projectPath = Join-Path $repoRoot $project
    $projectDirectory = Split-Path -Parent $projectPath
    [xml]$xml = Get-Content -LiteralPath $projectPath -Raw
    foreach ($node in $xml.SelectNodes('//*[local-name()="ClCompile" and @Include]')) {
        $sourcePath = [System.IO.Path]::GetFullPath((Join-Path $projectDirectory $node.Include))
        if (-not (Test-Path -LiteralPath $sourcePath)) { throw "Missing source: $sourcePath" }
        $source = $sourcePath.Substring($repoRoot.Length + 1).Replace('\', '/')
        [ordered]@{
            project = $project
            source = $source
            tasks = @(Get-MigrationTasks $project ([System.IO.Path]::GetFileNameWithoutExtension($sourcePath)))
        }
    }
}

$unclassified = @($units | Where-Object { $_.tasks -contains 'UNCLASSIFIED' })
if ($unclassified.Count -ne 0) {
    throw "Unclassified compilation units: $($unclassified.source -join ', '). Add explicit migration mappings."
}

$inventory = [ordered]@{
    format_version = 1
    branch = 'refactor/rust-migration'
    base_commit = '930f175'
    minimum_windows = 'Windows 10'
    projects = $projects
    compilation_unit_count = @($units).Count
    units = @($units | Sort-Object { $_.source })
}
$rendered = (($inventory | ConvertTo-Json -Depth 8) -replace "`r`n", "`n") + "`n"
$destination = Join-Path $repoRoot 'docs/rust-migration/source-inventory.json'
if ($Check) {
    if (-not (Test-Path -LiteralPath $destination)) { throw 'Migration source inventory is missing.' }
    $current = (Get-Content -LiteralPath $destination -Raw) -replace "`r`n", "`n"
    if ($current -cne $rendered) { throw 'Migration source inventory is stale. Run scripts/update-migration-inventory.ps1.' }
    Write-Output "Migration inventory covers $(@($units).Count) compilation units in $($projects.Count) projects."
}
else {
    [System.IO.Directory]::CreateDirectory((Split-Path -Parent $destination)) | Out-Null
    [System.IO.File]::WriteAllText($destination, $rendered, [System.Text.UTF8Encoding]::new($false))
    Write-Output "Updated migration inventory: $(@($units).Count) compilation units."
}
