param([string]$OutputRootDirectory = 'dist', [switch]$UseCurrentVersion)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    . "$PSScriptRoot\environment.ps1"
    if (-not $UseCurrentVersion) {
        & python "$PSScriptRoot\versioning.py"
        if ($LASTEXITCODE -ne 0) { throw 'Development version stamping failed' }
    }
    $taskManifest = Get-Content -LiteralPath 'Cargo.toml' -Raw
    $taskVersion = [regex]::Match($taskManifest, '(?m)^version = "(\d+\.\d+\.\d+(?:-dev\.[0-9a-f]{12})?)"').Groups[1].Value
    if (-not $taskVersion) { throw 'Cannot determine release version' }
    $taskName = "rolauncher-v$taskVersion"
    $taskOutput = if ([IO.Path]::IsPathRooted($OutputRootDirectory)) { [IO.Path]::GetFullPath($OutputRootDirectory) } else { [IO.Path]::GetFullPath((Join-Path $taskRoot $OutputRootDirectory)) }
    $taskBinaryZip = Join-Path $taskOutput "$taskName-windows-x64.zip"
    $taskSourceZip = Join-Path $taskOutput "$taskName-source.zip"
    $taskInstaller = Join-Path $taskOutput "$taskName-setup-x64.exe"
    $taskChecksums = Join-Path $taskOutput "$taskName-SHA256SUMS.txt"
    foreach ($taskArtifact in @($taskBinaryZip, $taskSourceZip, $taskInstaller, $taskChecksums)) {
        if (Test-Path -LiteralPath $taskArtifact) { throw "Release artifact already exists: $taskArtifact. Bump the version; do not overwrite a release." }
    }
    $taskDistribution = Join-Path $taskOutput $taskName
    if (Test-Path -LiteralPath $taskDistribution) { throw "Distribution directory already exists: $taskDistribution" }
    & "$PSScriptRoot\build.ps1" -UseCurrentVersion -OutputDirectory $taskDistribution
    Copy-Item -LiteralPath 'AGENTS.md' -Destination $taskDistribution
    Compress-Archive -LiteralPath $taskDistribution -DestinationPath $taskBinaryZip
    & python "$PSScriptRoot\package-development-source.py" $taskSourceZip
    if ($LASTEXITCODE -ne 0) { throw 'Source archive exclusion checks failed' }
    & "$PSScriptRoot\build-installer.ps1" -DistributionDirectory $taskDistribution -OutputDirectory $taskOutput
    $taskLines = foreach ($taskArtifact in @($taskBinaryZip, $taskSourceZip, $taskInstaller)) {
        $taskHash = Get-FileHash -LiteralPath $taskArtifact -Algorithm SHA256
        "$($taskHash.Hash.ToLowerInvariant())  $(Split-Path -Leaf $taskArtifact)"
    }
    $taskLines | Set-Content -LiteralPath $taskChecksums -Encoding ascii
    Write-Output "Packaged $taskName with source, notices and SHA256 checksums."
} finally { Pop-Location }
