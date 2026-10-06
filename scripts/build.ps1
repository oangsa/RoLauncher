param([switch]$UseCurrentVersion, [switch]$Debug, [string]$OutputDirectory = 'dist\rolauncher')
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    . "$PSScriptRoot\environment.ps1"
    if (-not $UseCurrentVersion) {
        & python "$PSScriptRoot\versioning.py"
        if ($LASTEXITCODE -ne 0) { throw 'Development version stamping failed' }
    }
    $taskArgs = @('build', '--locked')
    if (-not $Debug) { $taskArgs += '--release' }
    & cargo @taskArgs
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
    $taskProfile = if ($Debug) { 'debug' } else { 'release' }
    $taskDist = if ([IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory } else { Join-Path $taskRoot $OutputDirectory }
    New-Item -ItemType Directory -Force -Path $taskDist | Out-Null
    & "$PSScriptRoot\build-desktop.ps1" -OutputDirectory (Join-Path $taskDist 'desktop')
    if ($LASTEXITCODE -ne 0) { throw 'Desktop build failed' }
    Copy-Item -LiteralPath "target\$taskProfile\RoLauncher.exe" -Destination $taskDist
    Copy-Item -LiteralPath 'README.md','LICENSE','THIRD_PARTY_NOTICES.md' -Destination $taskDist
    Copy-Item -LiteralPath 'docs' -Destination $taskDist -Recurse -Force
    $taskLoader = Get-ChildItem -LiteralPath "target\$taskProfile\build" -Recurse -Filter WebView2Loader.dll | Where-Object { $_.Directory.Name -eq 'x64' } | Select-Object -First 1
    if (-not $taskLoader) { throw 'WebView2Loader.dll missing; cannot package a runnable application' }
    Copy-Item -LiteralPath $taskLoader.FullName -Destination $taskDist
    Copy-Item -LiteralPath $taskLoader.FullName -Destination "target\$taskProfile\WebView2Loader.dll"
    $taskLicenseDir = Join-Path $taskDist 'licenses'
    New-Item -ItemType Directory -Force -Path $taskLicenseDir | Out-Null
    if (Test-Path -LiteralPath 'licenses') { Copy-Item -Path 'licenses\*' -Destination $taskLicenseDir -Force }
    $taskHost = (& rustc -vV | Where-Object { $_ -like 'host: *' }) -replace '^host: ', ''
    $taskMetadata = & cargo metadata --locked --offline --filter-platform $taskHost --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Dependency license metadata could not be collected' }
    $taskLicenseMetadata = @()
    foreach ($taskPackage in $taskMetadata.packages) {
        $taskLicenseMetadata += [pscustomobject]@{ name=$taskPackage.name; version=$taskPackage.version; license=$taskPackage.license; repository=$taskPackage.repository }
        $taskPackageDir = Split-Path -Parent $taskPackage.manifest_path
        $taskPackageNotices = Get-ChildItem -LiteralPath $taskPackageDir -File | Where-Object { $_.Name -match '^(LICENSE|COPYING|NOTICE)' }
        if ($taskPackageNotices) {
            $taskDestination = Join-Path $taskLicenseDir ($taskPackage.name + '-' + $taskPackage.version)
            New-Item -ItemType Directory -Force -Path $taskDestination | Out-Null
            foreach ($taskNotice in $taskPackageNotices) { Copy-Item -LiteralPath $taskNotice.FullName -Destination $taskDestination }
        }
    }
    $taskLicenseMetadata | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskLicenseDir 'dependencies.json') -Encoding utf8
    Write-Output "Built $taskDist\RoLauncher.exe"
} finally { Pop-Location }
