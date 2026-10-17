param(
    [string]$Baseline = 'dist\rolauncher-v1.7.0\RoLauncher.exe',
    [string]$Candidate = 'target\release\RoLauncher.exe',
    [int]$Seconds = 20,
    [ValidateRange(1,10)][int]$Repeats = 3,
    [string]$Output = 'docs\hotfix-1.7.1-benchmark.json'
)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
. "$PSScriptRoot\environment.ps1"
$taskBenchmarks = @()
Push-Location -LiteralPath $taskRoot
try {
    for ($taskRound = 0; $taskRound -lt $Repeats; $taskRound++) {
        $taskLines = & cargo test --locked --release --lib hotfix_benchmark -- --ignored --nocapture --test-threads=1
        if ($LASTEXITCODE -ne 0) { throw 'Benchmark fixture failed' }
        foreach ($taskLine in $taskLines) {
            $taskOffset = $taskLine.IndexOf('HOTFIX_BENCH ')
            if ($taskOffset -ge 0) { $taskBenchmarks += ($taskLine.Substring($taskOffset + 13) | ConvertFrom-Json) }
        }
    }
} finally { Pop-Location }
$taskReports = @()
foreach ($taskVariant in @(@{name='1.7.0'; path=$Baseline}, @{name='1.7.1 development'; path=$Candidate})) {
    $taskExecutable = (Resolve-Path -LiteralPath (Join-Path $taskRoot $taskVariant.path)).Path
    $taskData = Join-Path $taskRoot ('target\hotfix-idle-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $taskData | Out-Null
    $taskProcess = Start-Process -FilePath $taskExecutable -ArgumentList @('--headless','--data-dir',('"' + $taskData + '"'),'--port','38479') -PassThru -WindowStyle Hidden
    try {
        Start-Sleep -Seconds 2
        if ($taskProcess.HasExited) { throw 'Isolated benchmark process exited before measurement' }
        $taskSample = Join-Path $taskData 'sample.json'
        & "$PSScriptRoot\benchmark.ps1" -ProcessId $taskProcess.Id -Seconds $Seconds -Output $taskSample | Out-Null
        $taskReport = Get-Content -LiteralPath $taskSample -Raw | ConvertFrom-Json
        $taskReports += [pscustomobject]@{ version=$taskVariant.name; binaryVersion=$taskProcess.MainModule.FileVersionInfo.ProductVersion; sample=$taskReport }
    } finally {
        if (-not $taskProcess.HasExited) { Stop-Process -Id $taskProcess.Id -Force }
        $taskProcess.WaitForExit()
    }
}
[pscustomobject]@{
    measuredAtUtc=[DateTime]::UtcNow.ToString('o')
    logicalProcessors=[Environment]::ProcessorCount
    microbenchmarks=$taskBenchmarks
    scope='Separate serial headless runs, empty isolated databases, zero Roblox clients; excludes desktop and active-client monitoring. Small CPU differences are not improvement percentages.'
    reports=$taskReports
} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $taskRoot $Output) -Encoding utf8
Get-Content -LiteralPath (Join-Path $taskRoot $Output)
