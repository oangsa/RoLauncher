param([string]$EvidenceDirectory = 'target/identity-poc-evidence')
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskPreviousRecovery = $env:ROLAUNCHER_IDENTITY_POC_RECOVERY_DIR
Push-Location -LiteralPath $taskRoot
try {
    . "$PSScriptRoot/environment.ps1"
    & python "$PSScriptRoot/test-identity-lab.py"
    if ($LASTEXITCODE -ne 0) { throw 'Portable receipt/source isolation checks failed' }
    $taskEvidence = [IO.Path]::GetFullPath((Join-Path $taskRoot $EvidenceDirectory))
    New-Item -ItemType Directory -Force -Path $taskEvidence | Out-Null
    $taskLocal = Join-Path $taskRoot ('target/identity-poc-local/' + [guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Force -Path $taskLocal | Out-Null
    $env:ROLAUNCHER_IDENTITY_POC_RECOVERY_DIR = Join-Path $taskLocal 'recovery'
    & cargo test --locked --example windows-device-identity
    if ($LASTEXITCODE -ne 0) { throw 'PoC contract tests failed' }
    & cargo build --locked --example windows-device-identity
    if ($LASTEXITCODE -ne 0) { throw 'PoC harness build failed' }
    $taskHarness = Join-Path $taskRoot 'target/debug/examples/windows-device-identity.exe'
    function Invoke-Poc([string[]]$Arguments) {
        $taskInfo = [Diagnostics.ProcessStartInfo]::new($taskHarness)
        $taskInfo.UseShellExecute = $false
        $taskInfo.CreateNoWindow = $true
        $taskInfo.RedirectStandardOutput = $true
        $taskInfo.RedirectStandardError = $true
        foreach ($taskArgument in $Arguments) { $taskInfo.ArgumentList.Add($taskArgument) }
        $taskProcess = [Diagnostics.Process]::Start($taskInfo)
        try {
            $taskOutput = $taskProcess.StandardOutput.ReadToEndAsync()
            $taskErrors = $taskProcess.StandardError.ReadToEndAsync()
            $taskProcess.WaitForExit()
            return [ordered]@{ exit_code=$taskProcess.ExitCode; output=$taskOutput.GetAwaiter().GetResult(); error=$taskErrors.GetAwaiter().GetResult() }
        } finally { $taskProcess.Dispose() }
    }
    $taskWindows = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
    $taskReport = [ordered]@{
        status='Diagnostic capability measurements; never standard-user enrollment acceptance'
        source_sha=(& git rev-parse HEAD)
        source_event=$env:GITHUB_EVENT_NAME
        windows=[ordered]@{ build=$taskWindows.CurrentBuildNumber; revision=$taskWindows.UBR; display_version=$taskWindows.DisplayVersion }
        context=(Invoke-Poc @('context'))
        capabilities=(Invoke-Poc @('capabilities'))
        cross_user='Not tested: requires genuine User B; follow manual procedure'
        reboot='Not tested: follow manual procedure'
        measurements=[ordered]@{}
    }
    foreach ($taskProvider in @('software','tpm')) {
        foreach ($taskScope in @('user')) {
            $taskReport.measurements["$taskProvider-$taskScope"] = Invoke-Poc @('key-probe',$taskProvider,$taskScope)
        }
        $taskReport.measurements["$taskProvider-machine"] = [ordered]@{ status='Pending manual disposable-lab provisioning; never attempted by hosted CI' }
    }
    foreach ($taskCandidate in @('B')) {
        $taskState = Join-Path $taskLocal ("private-local-public-state-$taskCandidate-" + [guid]::NewGuid().ToString() + '.json')
        $taskTrial = [ordered]@{ create=(Invoke-Poc @('measure',$taskCandidate,$taskState)) }
        # Failed creation explicitly accounts cleanup and preserves partial state locally.
        # Successful diagnostic state requires explicit disposal after all measurements.
        if ($taskTrial.create.exit_code -eq 0) {
            try {
                $taskTrial.restart = Invoke-Poc @('measure-check',$taskState)
                $taskTrial.inspect = Invoke-Poc @('inspect',$taskState)
                $taskTrial.owner_native_probe = Invoke-Poc @('probe',$taskState)
            } finally {
                $taskTrial.cleanup = Invoke-Poc @('cleanup',$taskState)
                if ($taskTrial.cleanup.exit_code -eq 0) { Remove-Item -LiteralPath $taskState }
            }
        }
        $taskReport.measurements["candidate-$taskCandidate"] = $taskTrial
    }
    foreach ($taskCandidate in @('A','C')) {
        $taskReport.measurements["candidate-$taskCandidate"] = [ordered]@{ status='Pending manual disposable-lab provisioning'; enrollment_accepted=$false }
    }
    $taskReport.recovery_accounting = Invoke-Poc @('recovery-summary')
    # Only this sanitized report is uploadable. Never upload PoC public state, SID references,
    # registry evidence, names or blobs. Keep failed-cleanup state local for recovery.
    $taskReport | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $taskEvidence 'measurements.json') -Encoding utf8
    Write-Output 'PoC measurements saved; inspect operation exit codes. Missing hardware/contexts are not passes.'
    foreach ($taskTrial in $taskReport.measurements.Values) {
        if ($taskTrial.Contains('cleanup') -and $taskTrial.cleanup.exit_code -ne 0) { throw 'PoC cleanup failed; retain local public state for recovery' }
        if ($taskTrial.Contains('error') -and $taskTrial.error.Contains('cleanup incomplete')) { throw 'PoC probe cleanup incomplete; retain local recovery receipts' }
    }
    if ($taskReport.recovery_accounting.exit_code -ne 0) { throw 'PoC recovery accounting unavailable' }
    $taskAccounting = $taskReport.recovery_accounting.output | ConvertFrom-Json
    if ($taskAccounting.incomplete -ne 0 -or $taskAccounting.intentionally_retained -ne 0) { throw 'PoC key cleanup unconfirmed; retain strictly local recovery receipts' }
} finally {
    $env:ROLAUNCHER_IDENTITY_POC_RECOVERY_DIR = $taskPreviousRecovery
    Pop-Location
}
