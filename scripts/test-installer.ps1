param([string]$HelperScript = "$PSScriptRoot\apply-update.ps1")
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    . "$PSScriptRoot\environment.ps1"
    $taskTest = Join-Path $taskRoot ('target\installer-test-' + [guid]::NewGuid().ToString('N'))
    $taskPayload = Join-Path $taskTest 'payload'
    $taskInstall = Join-Path $taskTest 'Installed app with spaces'
    $taskData = Join-Path $taskTest 'Preserved data with spaces'
    New-Item -ItemType Directory -Path $taskPayload,$taskInstall,$taskData | Out-Null
    # No Roblox, desktop or network integration: these processes only wait and record arguments.
    $taskFixture = @'
use std::{env, fs, thread, time::Duration};
fn main() {
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("--wait") {
        while std::path::Path::new(&args[2]).exists() { thread::sleep(Duration::from_millis(50)); }
    } else if let Some(index) = args.iter().position(|arg| arg == "--data-dir") {
        fs::write(std::path::Path::new(&args[index + 1]).join("restarted.txt"), args[1..].join("\n")).unwrap();
    }
}
'@
    $taskFixtureFile = Join-Path $taskTest 'fixture.rs'
    $taskFixture | Set-Content -LiteralPath $taskFixtureFile -Encoding utf8
    & rustc --edition 2024 --crate-name installer_fixture $taskFixtureFile -o (Join-Path $taskPayload 'RoLauncher.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Installer fixture compilation failed' }
    'Complete payload sentinel' | Set-Content -LiteralPath (Join-Path $taskPayload 'payload.txt')
    Copy-Item -LiteralPath (Join-Path $taskPayload 'RoLauncher.exe') -Destination $taskInstall
    Copy-Item -LiteralPath (Join-Path $taskPayload 'RoLauncher.exe') -Destination (Join-Path $taskTest 'DesktopFixture.exe')
    $taskSentinel = Join-Path $taskData 'accounts.json'
    'Fixture data must remain byte-for-byte unchanged' | Set-Content -LiteralPath $taskSentinel
    $taskDataHash = (Get-FileHash -LiteralPath $taskSentinel).Hash
    & "$PSScriptRoot\build-installer.ps1" -DistributionDirectory $taskPayload -OutputDirectory $taskTest -TestInstaller
    $taskVersion = [regex]::Match((Get-Content Cargo.toml -Raw), '(?m)^version = "(\d+\.\d+\.\d+)"').Groups[1].Value
    $taskInstaller = Join-Path $taskTest "rolauncher-v$taskVersion-setup-x64.exe"
    $taskHash = (Get-FileHash -LiteralPath $taskInstaller -Algorithm SHA256).Hash
    $taskPowerShell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $taskProcesses = @()
    function Wait-FixtureFile([string]$taskPath, [int]$taskSeconds = 15) {
        $taskDeadline = [DateTime]::UtcNow.AddSeconds($taskSeconds)
        while (-not (Test-Path -LiteralPath $taskPath)) {
            if ([DateTime]::UtcNow -ge $taskDeadline) { throw "Fixture timed out: $(Split-Path -Leaf $taskPath)" }
            Start-Sleep -Milliseconds 100
        }
    }
    foreach ($taskCase in @('bad-checksum', 'not-committed', 'install')) {
        $taskCaseDirectory = Join-Path $taskTest $taskCase
        New-Item -ItemType Directory -Path $taskCaseDirectory | Out-Null
        $taskParentFlag = Join-Path $taskCaseDirectory 'parent.wait'
        $taskDesktopFlag = Join-Path $taskCaseDirectory 'desktop.wait'
        'Wait' | Set-Content -LiteralPath $taskParentFlag,$taskDesktopFlag
        $taskParent = Start-Process -FilePath (Join-Path $taskInstall 'RoLauncher.exe') -ArgumentList @('--wait', ('"' + $taskParentFlag + '"')) -PassThru -WindowStyle Hidden
        $taskDesktop = Start-Process -FilePath (Join-Path $taskTest 'DesktopFixture.exe') -ArgumentList @('--wait', ('"' + $taskDesktopFlag + '"')) -PassThru -WindowStyle Hidden
        $taskProcesses += @($taskParent, $taskDesktop)
        $taskManifest = Join-Path $taskCaseDirectory 'request.json'
        @{
            Installer=$taskInstaller; Sha256=if ($taskCase -eq 'bad-checksum') { '0' * 64 } else { $taskHash }
            Version=$taskVersion; InstallDirectory=$taskInstall; DataDirectory=$taskData; Port=19371
            ParentId=$taskParent.Id; ParentStarted=$taskParent.StartTime.ToUniversalTime().Ticks
            DesktopId=$taskDesktop.Id; DesktopStarted=$taskDesktop.StartTime.ToUniversalTime().Ticks
        } | ConvertTo-Json | Set-Content -LiteralPath $taskManifest -Encoding utf8
        $taskHelper = Start-Process -FilePath $taskPowerShell -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File', ('"' + $HelperScript + '"'), '-Manifest', ('"' + $taskManifest + '"')) -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskCaseDirectory 'helper.txt') -RedirectStandardError (Join-Path $taskCaseDirectory 'helper-error.txt')
        $taskProcesses += $taskHelper
        $taskReady = Join-Path $taskCaseDirectory 'ready'
        $taskResultFile = Join-Path $taskCaseDirectory 'result.json'
        if ($taskCase -eq 'bad-checksum') {
            Wait-FixtureFile $taskResultFile
            if (Test-Path -LiteralPath $taskReady) { throw 'A bad checksum was allowed to prepare installation' }
        } else {
            try { Wait-FixtureFile $taskReady } catch { if (Test-Path -LiteralPath $taskResultFile) { Write-Output (Get-Content -LiteralPath $taskResultFile -Raw) }; throw }
            if ($taskCase -eq 'install') {
                'Apply' | Set-Content -LiteralPath (Join-Path $taskCaseDirectory 'commit')
                Start-Sleep -Milliseconds 300
                if (Test-Path -LiteralPath (Join-Path $taskInstall 'payload.txt')) { throw 'Installer ran while the app was still open' }
                Remove-Item -LiteralPath $taskDesktopFlag
                $taskDesktop.WaitForExit()
                Start-Sleep -Milliseconds 300
                if (Test-Path -LiteralPath (Join-Path $taskInstall 'payload.txt')) { throw 'Installer did not wait for the supervisor' }
                Remove-Item -LiteralPath $taskParentFlag
                $taskParent.WaitForExit()
            }
            Wait-FixtureFile $taskResultFile 30
        }
        if (-not $taskHelper.WaitForExit(15000)) { throw 'Helper did not exit cleanly' }
        $taskResult = Get-Content -LiteralPath $taskResultFile -Raw | ConvertFrom-Json
        if ($taskCase -eq 'install') {
            if ($taskResult.status -ne 'installed') { throw 'Isolated installer handoff failed' }
            Wait-FixtureFile (Join-Path $taskData 'restarted.txt')
            $taskRestart = Get-Content -LiteralPath (Join-Path $taskData 'restarted.txt')
            if ($taskRestart[1] -ne $taskData -or $taskRestart[3] -ne '19371') { throw 'Restart did not preserve data directory and port' }
            if (-not (Test-Path -LiteralPath (Join-Path $taskInstall 'payload.txt'))) { throw 'Complete payload not installed' }
            if ((Get-FileHash -LiteralPath (Join-Path $taskInstall 'RoLauncher.exe')).Hash -ne (Get-FileHash -LiteralPath (Join-Path $taskPayload 'RoLauncher.exe')).Hash) { throw 'Installed executable mismatch' }
        } else {
            if ($taskResult.status -ne 'failed' -or $taskParent.HasExited -or $taskDesktop.HasExited) { throw 'Rejected handoff affected running apps' }
            if (Test-Path -LiteralPath (Join-Path $taskInstall 'payload.txt')) { throw 'Rejected handoff installed files' }
            Remove-Item -LiteralPath $taskParentFlag,$taskDesktopFlag
            $taskParent.WaitForExit(); $taskDesktop.WaitForExit()
        }
        if ((Get-FileHash -LiteralPath $taskSentinel).Hash -ne $taskDataHash) { throw 'Update changed saved data' }
    }
    Write-Output 'Isolated installer tests passed: checksum rejection, two-phase cancellation, both process exit handles, complete installation, automatic restart, preserved data directory/port/data.'
} finally {
    # Only these owned fixture handles may be stopped; no saved application or Roblox process is touched.
    foreach ($taskProcess in $taskProcesses) {
        if (-not $taskProcess.HasExited) { $taskProcess.Kill(); $taskProcess.WaitForExit() }
        $taskProcess.Dispose()
    }
    Pop-Location
}
