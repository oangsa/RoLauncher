param([Parameter(Mandatory=$true)][string]$DistributionDirectory, [string]$OutputDirectory = 'dist', [switch]$TestInstaller)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    $taskVersion = [regex]::Match((Get-Content Cargo.toml -Raw), '(?m)^version = "(\d+\.\d+\.\d+)"').Groups[1].Value
    if (-not $taskVersion) { throw 'Cannot determine installer version' }
    $taskPayload = (Resolve-Path -LiteralPath $DistributionDirectory).Path
    if (-not (Test-Path -LiteralPath (Join-Path $taskPayload 'RoLauncher.exe'))) { throw 'Installer payload is missing RoLauncher.exe' }
    $taskOutput = if ([IO.Path]::IsPathRooted($OutputDirectory)) { [IO.Path]::GetFullPath($OutputDirectory) } else { [IO.Path]::GetFullPath((Join-Path $taskRoot $OutputDirectory)) }
    New-Item -ItemType Directory -Force -Path $taskOutput | Out-Null
    $taskInstaller = Join-Path $taskOutput "rolauncher-v$taskVersion-setup-x64.exe"
    if (Test-Path -LiteralPath $taskInstaller) { throw 'Installer already exists; do not replace release artifacts' }
    $taskCompiler = @((Join-Path $taskRoot '.tools\inno\ISCC.exe'),
        'C:\Program Files (x86)\Inno Setup 6\ISCC.exe', 'C:\Program Files\Inno Setup 7\ISCC.exe') |
        Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $taskCompiler) { $taskCompiler = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source }
    if (-not $taskCompiler) { throw 'Inno Setup compiler not found. Install Inno Setup 6.7+ or put ISCC.exe on PATH.' }
    $taskParts = $taskVersion.Split('.')
    $taskArgs = @('/Qp', "/DVersion=$taskVersion", "/DVersionMajor=$($taskParts[0])", "/DVersionMinor=$($taskParts[1])",
        "/DVersionPatch=$($taskParts[2])", "/DPayload=$taskPayload", "/O$taskOutput")
    if ($TestInstaller) { $taskArgs += '/DTestInstaller=1' }
    $taskArgs += (Join-Path $taskRoot 'installer\RoLauncher.iss')
    & $taskCompiler @taskArgs
    if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed' }
    if (-not (Test-Path -LiteralPath $taskInstaller)) { throw 'Installer output is missing' }
    Write-Output "Built $taskInstaller"
} finally { Pop-Location }
