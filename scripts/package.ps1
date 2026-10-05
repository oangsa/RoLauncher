$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    $taskManifest = Get-Content -LiteralPath 'Cargo.toml' -Raw
    $taskVersion = [regex]::Match($taskManifest, '(?m)^version = "(\d+\.\d+\.\d+)"').Groups[1].Value
    if (-not $taskVersion) { throw 'Cannot determine release version' }
    $taskName = "rolauncher-v$taskVersion"
    $taskBinaryZip = Join-Path $taskRoot "dist\$taskName-windows-x64.zip"
    $taskSourceZip = Join-Path $taskRoot "dist\$taskName-source.zip"
    $taskChecksums = Join-Path $taskRoot "dist\$taskName-SHA256SUMS.txt"
    foreach ($taskArtifact in @($taskBinaryZip, $taskSourceZip, $taskChecksums)) {
        if (Test-Path -LiteralPath $taskArtifact) { throw "Release artifact already exists: $taskArtifact. Bump the version; do not overwrite a release." }
    }
    & "$PSScriptRoot\build.ps1" -OutputDirectory "dist\$taskName"
    $taskDistribution = Join-Path $taskRoot "dist\$taskName"
    Copy-Item -LiteralPath 'AGENTS.md' -Destination $taskDistribution
    Compress-Archive -LiteralPath $taskDistribution -DestinationPath $taskBinaryZip
    $taskSource = Join-Path $taskRoot ('target\source-package-' + [guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $taskSource | Out-Null
    foreach ($taskItem in @('Cargo.toml','Cargo.lock','README.md','AGENTS.md','LICENSE','THIRD_PARTY_NOTICES.md','.gitignore','src','examples','scripts','docs','licenses')) {
        Copy-Item -LiteralPath $taskItem -Destination $taskSource -Recurse
    }
    foreach ($taskProject in @('desktop','desktop.tests')) {
        $taskProjectSource = Join-Path $taskSource $taskProject
        New-Item -ItemType Directory -Path $taskProjectSource | Out-Null
        Get-ChildItem -LiteralPath $taskProject -File | Copy-Item -Destination $taskProjectSource
    }
    Get-ChildItem -LiteralPath $taskSource | Compress-Archive -DestinationPath $taskSourceZip
    $taskLines = foreach ($taskArtifact in @($taskBinaryZip, $taskSourceZip)) {
        $taskHash = Get-FileHash -LiteralPath $taskArtifact -Algorithm SHA256
        "$($taskHash.Hash.ToLowerInvariant())  $(Split-Path -Leaf $taskArtifact)"
    }
    $taskLines | Set-Content -LiteralPath $taskChecksums -Encoding ascii
    Write-Output "Packaged $taskName with source, notices and SHA256 checksums."
} finally { Pop-Location }
