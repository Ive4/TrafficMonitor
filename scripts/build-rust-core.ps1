param(
    [string]$Cargo = 'cargo',
    [Parameter(Mandatory)]
    [ValidateSet('x86_64-pc-windows-msvc', 'i686-pc-windows-msvc', 'arm64ec-pc-windows-msvc')]
    [string]$Target,
    [Parameter(Mandatory)]
    [ValidateSet('dev', 'release')]
    [string]$Profile
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
& $Cargo build --manifest-path (Join-Path $repoRoot 'Cargo.toml') --target-dir (Join-Path $repoRoot 'target') --package traffic-monitor-core --target $Target --profile $Profile --locked --offline -j 1
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$profileDirectory = if ($Profile -eq 'dev') { 'debug' } else { 'release' }
$output = Join-Path $repoRoot "target/$Target/$profileDirectory"
$validatedAt = [DateTime]::UtcNow
foreach ($name in @('traffic_monitor_core.dll', 'traffic_monitor_core.dll.lib')) {
    $artifact = Join-Path $output $name
    if (-not (Test-Path -LiteralPath $artifact)) { throw "Cargo did not produce $artifact" }
    # Cargo preserves unchanged artifact timestamps after manifest-only edits.
    # Mark both outputs as validated so MSBuild can skip the next custom build.
    [System.IO.File]::SetLastWriteTimeUtc($artifact, $validatedAt)
}
