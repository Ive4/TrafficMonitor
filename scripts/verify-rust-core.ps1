param(
    [string]$Cargo = 'cargo',
    [string]$MSBuild,
    [ValidateSet('x64', 'Win32')]
    [string]$Platform = 'x64'
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$cargoExe = (Get-Command $Cargo -ErrorAction Stop).Source
$rustTarget = if ($Platform -eq 'Win32') { 'i686-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }

if (-not $MSBuild) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) {
        throw 'MSBuild not found. Supply -MSBuild with its executable path.'
    }
    $MSBuild = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find 'MSBuild\**\Bin\MSBuild.exe' | Select-Object -First 1
    if (-not $MSBuild) {
        throw 'Install the Visual Studio C++ build tools or supply -MSBuild.'
    }
}

function Invoke-Checked {
    param([string]$Executable, [string[]]$Arguments)
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Executable failed with exit code $LASTEXITCODE."
    }
}

Push-Location $repoRoot
try {
    & (Join-Path $PSScriptRoot 'update-migration-inventory.ps1') -Check
    Invoke-Checked $cargoExe @('fmt', '--all', '--', '--check')
    Invoke-Checked $cargoExe @('clippy', '--workspace', '--all-targets', '--target', $rustTarget, '--locked', '--offline', '-j', '1', '--', '-D', 'warnings')
    Invoke-Checked $cargoExe @('test', '--workspace', '--target', $rustTarget, '--locked', '--offline', '-j', '1')

    foreach ($configuration in @('Debug', 'Release')) {
        Invoke-Checked $MSBuild @(
            'tests/rust-core-ffi/RustCoreFfi.vcxproj', '/nologo', '/m:1', '/v:minimal',
            "/p:Configuration=$configuration", "/p:Platform=$Platform", "/p:TrafficMonitorCargo=$cargoExe"
        )
        $testExe = Join-Path $repoRoot "target/ffi-smoke/$Platform/$configuration/rust-core-ffi.exe"
        Invoke-Checked $testExe @()
    }
}
finally {
    Pop-Location
}
