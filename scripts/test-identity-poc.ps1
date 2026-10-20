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
    $taskTarget = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR) } else { Join-Path $taskRoot 'target' }
    $taskHarness = Join-Path $taskTarget 'debug/examples/windows-device-identity.exe'
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
    foreach ($taskCandidate in @('A','B','C')) {
        $taskReport.measurements["candidate-$taskCandidate"] = [ordered]@{ status='Pending human-confirmed disposable-lab test; no native mutations in CI'; enrollment_accepted=$false }
    }
    $taskReport.recovery_accounting = Invoke-Poc @('recovery-summary')
    # Only this sanitized report is uploadable. Never upload PoC public state, SID references,
    # registry evidence, names or blobs. Keep failed-cleanup state local for recovery.
    $taskReport | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $taskEvidence 'measurements.json') -Encoding utf8
    Write-Output 'PoC measurements saved; inspect operation exit codes. Missing hardware/contexts are not passes.'
    if ($taskReport.recovery_accounting.exit_code -ne 0) { throw 'PoC recovery accounting unavailable' }
    $taskAccounting = $taskReport.recovery_accounting.output | ConvertFrom-Json
    if ($taskAccounting.incomplete -ne 0 -or $taskAccounting.intentionally_retained -ne 0) { throw 'PoC key cleanup unconfirmed; retain strictly local recovery receipts' }
} finally {
    $env:ROLAUNCHER_IDENTITY_POC_RECOVERY_DIR = $taskPreviousRecovery
    Pop-Location
}
