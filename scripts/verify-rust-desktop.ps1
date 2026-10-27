param(
    [ValidateSet('x64','x86')][string]$Architecture = 'x64',
    [string]$Cargo = 'cargo',
    [ValidatePattern('^[A-Za-z0-9._-]+\.exe$')][string]$ExecutableName='TrafficMonitor.exe',
    [switch]$DesktopSmoke
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$rustTarget = if ($Architecture -eq 'x86') { 'i686-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
$cargoExe = (Get-Command $Cargo -ErrorAction Stop).Source
function Invoke-Checked([string[]]$Arguments) {
    & $cargoExe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed: $LASTEXITCODE" }
}
Push-Location $repoRoot
try {
    & (Join-Path $PSScriptRoot 'update-migration-inventory.ps1') -Check
    Invoke-Checked @('fmt','--all','--','--check')
    Invoke-Checked @('clippy','--workspace','--all-targets','--target',$rustTarget,'--locked','--offline','-j','1','--','-D','warnings')
    Invoke-Checked @('test','--workspace','--target',$rustTarget,'--locked','--offline','-j','1')
    & (Join-Path $PSScriptRoot 'build-rust-release.ps1') -Architecture $Architecture -Cargo $cargoExe -ExecutableName $ExecutableName
    $executable = Join-Path $repoRoot "Bin/Rust/$Architecture/$ExecutableName"
    $data = Join-Path $repoRoot "target/desktop-smoke/$Architecture-$([DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff'))"
    $headlessInfo=[Diagnostics.ProcessStartInfo]::new($executable)
    $headlessInfo.UseShellExecute=$false
    $headlessInfo.RedirectStandardOutput=$true
    $headlessInfo.RedirectStandardError=$true
    foreach($value in @('--data-dir',$data,'--headless','--no-plugins','--samples','3')){$headlessInfo.ArgumentList.Add($value)}
    $headless=[Diagnostics.Process]::Start($headlessInfo)
    if(-not $headless.WaitForExit(20000)){$headless.Kill();throw 'Owned sampling process timed out'}
    $headless.StandardOutput.ReadToEnd()
    $headless.StandardError.ReadToEnd()
    if($headless.ExitCode -ne 0){throw 'Real sampling smoke failed'}
    foreach($name in @('config.ini','history_traffic.dat','history_traffic.dat.bak')){if(-not(Test-Path -LiteralPath (Join-Path $data $name))){throw "Sampling did not persist $name"}}
    if ($DesktopSmoke) {
        Invoke-Checked @('test','-p','traffic-monitor-desktop-ui','--target',$rustTarget,'--locked','--offline','-j','1','--','--ignored','--test-threads=1')
        $startInfo = [Diagnostics.ProcessStartInfo]::new($executable)
        $startInfo.UseShellExecute = $false
        foreach ($value in @('--data-dir',$data,'--exit-after-ms','4000','--no-plugins','--taskbar')) { $startInfo.ArgumentList.Add($value) }
        $process = [Diagnostics.Process]::Start($startInfo)
        if (-not $process.WaitForExit(20000)) { $process.Kill(); throw 'Owned desktop validation process timed out' }
        if ($process.ExitCode -ne 0) { throw "Desktop smoke failed: $($process.ExitCode)" }
    }
}
finally { Pop-Location }
