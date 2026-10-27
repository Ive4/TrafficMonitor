param(
    [ValidateSet('x64', 'x86', 'arm64ec')][string]$Architecture = 'x64',
    [string]$Cargo = 'cargo',
    [ValidatePattern('^[A-Za-z0-9._-]+\.exe$')][string]$ExecutableName='TrafficMonitor.exe',
    [switch]$Online
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$targets = @{x64='x86_64-pc-windows-msvc'; x86='i686-pc-windows-msvc'; arm64ec='arm64ec-pc-windows-msvc'}
$rustTarget = $targets[$Architecture]
$cargoExe = (Get-Command $Cargo -ErrorAction Stop).Source
Push-Location $repoRoot
try {
    $arguments = @('build','-p','traffic-monitor-native','--bin','TrafficMonitor','--release','--target',$rustTarget,'--locked','-j','1')
    if (-not $Online) { $arguments += '--offline' }
    & $cargoExe @arguments
    if ($LASTEXITCODE -ne 0) { throw "Rust build failed: $LASTEXITCODE" }
    $destination = Join-Path $repoRoot "Bin/Rust/$Architecture"
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    Copy-Item -LiteralPath "target/$rustTarget/release/TrafficMonitor.exe" -Destination (Join-Path $destination $ExecutableName) -Force
    foreach ($directory in @('skins','language','res')) {
        $sourceDirectory=Join-Path $repoRoot "TrafficMonitor/$directory"
        foreach($resource in Get-ChildItem -LiteralPath $sourceDirectory -File -Recurse){
            $relative=$resource.FullName.Substring($sourceDirectory.Length).TrimStart('\','/')
            $targetResource=Join-Path $destination (Join-Path $directory $relative)
            if((Test-Path -LiteralPath $targetResource) -and ((Get-Item -LiteralPath $targetResource).Length -eq $resource.Length) -and ((Get-FileHash -LiteralPath $targetResource).Hash -eq (Get-FileHash -LiteralPath $resource.FullName).Hash)){continue}
            New-Item -ItemType Directory -Path (Split-Path -Parent $targetResource) -Force | Out-Null
            Copy-Item -LiteralPath $resource.FullName -Destination $targetResource -Force
        }
    }
    Copy-Item -LiteralPath (Join-Path $repoRoot 'LICENSE') -Destination $destination -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'docs/rust-migration/RUST-README.txt') -Destination (Join-Path $destination 'README.txt') -Force
    $metadata = & $cargoExe metadata --format-version 1 --locked --offline | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Dependency metadata failed' }
    $thirdParty = @($metadata.packages | Where-Object source)
    $licenseDirectory = Join-Path $destination 'third-party-licenses'
    New-Item -ItemType Directory -Path $licenseDirectory -Force | Out-Null
    $notices = @('Third-party Rust dependencies. Original license texts are included below.', '')
    foreach ($package in $thirdParty) {
        $crateDirectory = Split-Path -Parent $package.manifest_path
        $notices += "$($package.name) $($package.version): $($package.license)"
        $licenses = @(Get-ChildItem -LiteralPath $crateDirectory -File | Where-Object { $_.Name -match '^(?i:license|copying|notice)' })
        if ($licenses.Count -eq 0) { throw "No license file found for $($package.name)" }
        foreach ($license in $licenses) {
            Copy-Item -LiteralPath $license.FullName -Destination (Join-Path $licenseDirectory "$($package.name)-$($package.version)-$($license.Name)") -Force
        }
    }
    $rustcCommand = if ($env:RUSTC) { $env:RUSTC } else { Join-Path (Split-Path -Parent $cargoExe) 'rustc.exe' }
    if (-not (Test-Path -LiteralPath $rustcCommand)) { $rustcCommand = (Get-Command rustc -ErrorAction Stop).Source }
    $sysroot = & $rustcCommand --print sysroot
    if ($LASTEXITCODE -ne 0) { throw 'Rust sysroot lookup failed' }
    $rustDocumentation = Join-Path $sysroot 'share/doc/rust'
    $rustLibraryCopyright = Join-Path $rustDocumentation 'COPYRIGHT-library.html'
    if (Test-Path -LiteralPath $rustLibraryCopyright) {
        $rustNotices = Join-Path $licenseDirectory 'rust-standard-library'
        New-Item -ItemType Directory -Path $rustNotices -Force | Out-Null
        Copy-Item -LiteralPath $rustLibraryCopyright -Destination $rustNotices -Force
        if (Test-Path -LiteralPath (Join-Path $rustDocumentation 'licenses')) { Copy-Item -LiteralPath (Join-Path $rustDocumentation 'licenses') -Destination $rustNotices -Recurse -Force }
    }
    $notices | Set-Content -LiteralPath (Join-Path $destination 'THIRD-PARTY-NOTICES.txt') -Encoding utf8
    $pdb = Join-Path $repoRoot "target/$rustTarget/release/TrafficMonitor.pdb"
    if (Test-Path -LiteralPath $pdb) { Copy-Item -LiteralPath $pdb -Destination $destination -Force }
    $executable = Join-Path $destination $ExecutableName
    $file=Get-Item -LiteralPath $executable
    $artifact=[ordered]@{architecture=$Architecture;target=$rustTarget;version=$file.VersionInfo.ProductVersion;executable=$file.FullName;bytes=$file.Length;sha256=(Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash;third_party_packages=$thirdParty.Count}
    $artifact | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $destination 'artifact-manifest.json') -Encoding utf8
    [pscustomobject]$artifact | Format-List
}
finally { Pop-Location }
