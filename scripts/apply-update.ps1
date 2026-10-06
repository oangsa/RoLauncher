param([Parameter(Mandatory=$true)][string]$Manifest)
$ErrorActionPreference = 'Stop'
$taskDirectory = Split-Path -Parent ([IO.Path]::GetFullPath($Manifest))
$taskRequest = $null
$taskReady = $false
$taskCanRestart = $false
$taskStage = 'request'
function Get-OriginalProcess([int]$taskId, [long]$taskStarted) {
    $taskProcess = Get-Process -Id $taskId -ErrorAction SilentlyContinue
    if ($null -eq $taskProcess) { return $null }
    if ($taskProcess.StartTime.ToUniversalTime().Ticks -ne $taskStarted) {
        $taskProcess.Dispose()
        return $null
    }
    # Force Windows to retain this process's exit handle before its PID can be reused.
    $null = $taskProcess.Handle
    return $taskProcess
}
function Get-InstallerHash([string]$taskPath) {
    # Use .NET directly: Windows PowerShell's Get-FileHash module may be absent from a launching shell's module path.
    $taskStream = [IO.File]::OpenRead($taskPath)
    $taskHasher = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($taskHasher.ComputeHash($taskStream)).Replace('-', '') }
    finally { $taskHasher.Dispose(); $taskStream.Dispose() }
}
try {
    $taskRequest = Get-Content -LiteralPath $Manifest -Raw | ConvertFrom-Json
    if ($taskRequest.Version -notmatch '^\d+\.\d+\.\d+(?:-dev\.[0-9a-f]{12})?$' -or $taskRequest.Sha256 -notmatch '^[a-fA-F0-9]{64}$' -or
        $taskRequest.Port -lt 1 -or $taskRequest.Port -gt 65535) { throw 'Invalid update request' }
    $taskStage = 'paths'
    $taskInstaller = [IO.Path]::GetFullPath([string]$taskRequest.Installer)
    $taskInstallDirectory = [IO.Path]::GetFullPath([string]$taskRequest.InstallDirectory)
    $taskExecutable = Join-Path $taskInstallDirectory 'RoLauncher.exe'
    if ([IO.Path]::GetFileName($taskInstaller) -ne ('rolauncher-v' + $taskRequest.Version + '-setup-x64.exe') -or
        -not (Test-Path -LiteralPath $taskExecutable -PathType Leaf)) { throw 'Invalid installer or installation path' }
    $taskStage = 'checksum'
    if ((Get-InstallerHash $taskInstaller) -ne $taskRequest.Sha256) { throw 'Installer checksum mismatch' }
    $taskStage = 'identity'
    $taskParent = Get-OriginalProcess $taskRequest.ParentId $taskRequest.ParentStarted
    $taskDesktop = Get-OriginalProcess $taskRequest.DesktopId $taskRequest.DesktopStarted
    if ($null -eq $taskParent -or $null -eq $taskDesktop -or $taskParent.MainModule.FileName -ne $taskExecutable) { throw 'Unexpected supervisor identity' }
    Set-Content -LiteralPath (Join-Path $taskDirectory 'ready') -Value 'Ready' -Encoding ascii
    $taskReady = $true
    $taskStage = 'confirmation'
    $taskCommitDeadline = [DateTime]::UtcNow.AddSeconds(20)
    while (-not (Test-Path -LiteralPath (Join-Path $taskDirectory 'commit') -PathType Leaf)) {
        if ([DateTime]::UtcNow -ge $taskCommitDeadline) { throw 'Update was not confirmed by the desktop' }
        Start-Sleep -Milliseconds 100
    }
    $taskStage = 'exit'
    foreach ($taskProcess in @($taskDesktop, $taskParent)) {
        if ($null -ne $taskProcess) {
            if (-not $taskProcess.WaitForExit(60000)) { throw 'RoLauncher did not exit before installation' }
            $taskProcess.Dispose()
        }
    }
    $taskCanRestart = $true
    if ((Get-InstallerHash $taskInstaller) -ne $taskRequest.Sha256) { throw 'Installer checksum mismatch' }
    # The app has exited itself. Never close Roblox clients or force-close other applications.
    $taskArguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOCLOSEAPPLICATIONS', '/NORESTARTAPPLICATIONS',
        ('/DIR="' + $taskInstallDirectory + '"'))
    $taskStage = 'installer'
    $taskSetup = Start-Process -FilePath $taskInstaller -ArgumentList $taskArguments -PassThru -WindowStyle Hidden
    $taskSetup.WaitForExit()
    $taskExitCode = $taskSetup.ExitCode
    $taskSetup.Dispose()
    if ($taskExitCode -ne 0) { throw ('Installer failed with exit code ' + $taskExitCode) }
    @{ status='installed'; version=$taskRequest.Version } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskDirectory 'result.json') -Encoding utf8
} catch {
    # Only a fixed result is saved; no command lines, credentials or arbitrary exception text.
    @{ status='failed'; stage=$taskStage; version=if ($null -ne $taskRequest) { $taskRequest.Version } else { '' }; message='The update could not be installed. Download it again or run the official installer.' } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskDirectory 'result.json') -Encoding utf8
} finally {
    if ($taskReady -and $taskCanRestart -and (Test-Path -LiteralPath $taskExecutable -PathType Leaf)) {
        # Quoted Windows arguments must double trailing backslashes, including a root data directory.
        $taskDataArgument = [regex]::Replace([string]$taskRequest.DataDirectory, '(\\+)$', '$1$1')
        $taskLaunchArguments = @('--data-dir', ('"' + $taskDataArgument + '"'), '--port', [string]$taskRequest.Port)
        Start-Process -FilePath $taskExecutable -ArgumentList $taskLaunchArguments -WorkingDirectory $taskInstallDirectory -WindowStyle Hidden | Out-Null
    }
}
