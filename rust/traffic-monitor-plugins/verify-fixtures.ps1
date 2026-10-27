param(
    [string[]]$Targets = @('x86_64-pc-windows-msvc', 'i686-pc-windows-msvc'),
    [string]$Cargo = 'cargo',
    [string]$Rustc = 'rustc',
    [switch]$RustOnly
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$cargoCommand = (Get-Command $Cargo -CommandType Application -ErrorAction Stop).Source
$rustcCommand = (Get-Command $Rustc -CommandType Application -ErrorAction Stop).Source
foreach ($target in $Targets) {
    $output = Join-Path $repo "target/plugin-fixtures/$target"
    New-Item -ItemType Directory -Force $output | Out-Null
    $dll = Join-Path $output 'rust_abi_fixture.dll'
    & $rustcCommand --crate-name rust_abi_fixture --crate-type cdylib --edition 2024 --target $target -C panic=abort (Join-Path $PSScriptRoot 'fixtures/rust_plugin.rs') -o $dll
    if ($LASTEXITCODE -ne 0) { throw "Rust fixture compile failed: $target" }
    $previous = $env:TM_RUST_PLUGIN
    try {
        $env:TM_RUST_PLUGIN = $dll
        $arguments = @('test','--manifest-path',(Join-Path $repo 'Cargo.toml'),'-p','traffic-monitor-plugins','--offline','--locked','-j','1','--target',$target,'--test','fixtures')
        if ($RustOnly) { $arguments += 'rust_abi_fixture_all_versions_and_lifecycle' }
        $arguments += @('--','--ignored','--test-threads=1')
        & $cargoCommand @arguments
        if ($LASTEXITCODE -ne 0) { throw "Plugin ABI fixture tests failed: $target" }
    } finally { $env:TM_RUST_PLUGIN = $previous }
}
