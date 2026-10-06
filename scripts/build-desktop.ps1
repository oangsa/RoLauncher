param([string]$OutputDirectory = 'target\desktop', [switch]$Smoke)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    . "$PSScriptRoot\environment.ps1"
    $taskVersion = [regex]::Match((Get-Content Cargo.toml -Raw), '(?m)^version = "(\d+\.\d+\.\d+(?:-dev\.[0-9a-f]{12})?)"').Groups[1].Value
    & dotnet restore desktop/RoLauncher.Desktop.csproj --locked-mode --configfile desktop/NuGet.Config -p:Platform=x64
    if ($LASTEXITCODE -ne 0) { throw 'WinUI dependency restore failed' }
    $taskArgs = @('publish', 'desktop/RoLauncher.Desktop.csproj', '--no-restore', '-c', 'Release', '-p:Platform=x64', "-p:Version=$taskVersion", '-o', $OutputDirectory)
    if ($Smoke) { $taskArgs += '-p:UiSmoke=true' }
    & dotnet @taskArgs
    if ($LASTEXITCODE -ne 0) { throw 'WinUI build failed' }
    if (-not (Test-Path -LiteralPath (Join-Path $OutputDirectory 'RoLauncher.Desktop.pri'))) { throw 'Compiled XAML resources are missing' }
    $taskLicenses = Join-Path $OutputDirectory 'licenses'
    New-Item -ItemType Directory -Force -Path $taskLicenses | Out-Null
    $taskAssets = Get-Content desktop/obj/project.assets.json -Raw | ConvertFrom-Json
    $taskMetadata = foreach ($taskLibrary in $taskAssets.libraries.PSObject.Properties) {
        if ($taskLibrary.Value.type -ne 'package') { continue }
        $taskPackage = Join-Path $env:NUGET_PACKAGES $taskLibrary.Value.path
        $taskNuspec = Get-ChildItem -LiteralPath $taskPackage -Filter '*.nuspec' -File | Select-Object -First 1
        if (-not $taskNuspec) { throw "Package metadata missing: $($taskLibrary.Name)" }
        [xml]$taskXml = Get-Content -LiteralPath $taskNuspec.FullName -Raw
        $taskDestination = Join-Path $taskLicenses $taskLibrary.Name.Replace('/', '-')
        New-Item -ItemType Directory -Force -Path $taskDestination | Out-Null
        Copy-Item -LiteralPath $taskNuspec.FullName -Destination $taskDestination
        $taskNotices = Get-ChildItem -LiteralPath $taskPackage -File | Where-Object { $_.Name -match '(?i)(license|copying|notice)' }
        foreach ($taskNotice in $taskNotices) { Copy-Item -LiteralPath $taskNotice.FullName -Destination $taskDestination }
        $taskLicense = $taskXml.package.metadata.license
        if ($taskLicense -and $taskLicense.type -eq 'file') {
            $taskLicenseFile = Join-Path $taskPackage $taskLicense.InnerText
            if (-not (Test-Path -LiteralPath $taskLicenseFile)) { throw "Package license file missing: $($taskLibrary.Name)" }
            Copy-Item -LiteralPath $taskLicenseFile -Destination $taskDestination -Force
        }
        [pscustomobject]@{ package=$taskLibrary.Name; license=$taskLicense.InnerText; licenseUrl=$taskXml.package.metadata.licenseUrl }
    }
    $taskMetadata | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskLicenses 'dependencies.json') -Encoding utf8
} finally { Pop-Location }
