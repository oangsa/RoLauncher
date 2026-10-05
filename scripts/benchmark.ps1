param([Parameter(Mandatory=$true)][int]$ProcessId, [int]$Seconds = 30, [string]$Output = 'benchmark.json')
$ErrorActionPreference = 'Stop'
if ($Seconds -lt 1 -or $Seconds -gt 300) { throw 'Seconds must be 1–300' }
$taskProcess = Get-Process -Id $ProcessId
$taskCpuBefore = $taskProcess.TotalProcessorTime.TotalSeconds
$taskClock = [Diagnostics.Stopwatch]::StartNew()
$taskSamples = @()
for ($taskIndex = 0; $taskIndex -lt $Seconds; $taskIndex++) {
    Start-Sleep -Seconds 1
    $taskProcess.Refresh()
    if ($taskProcess.HasExited) { throw 'Target process exited during measurement' }
    $taskSamples += [pscustomobject]@{ workingSetBytes=$taskProcess.WorkingSet64; privateBytes=$taskProcess.PrivateMemorySize64 }
}
$taskProcess.Refresh()
$taskReport = [pscustomobject]@{
    processId=$ProcessId
    elapsedSeconds=$taskClock.Elapsed.TotalSeconds
    cpuPercentOfMachine=100 * ($taskProcess.TotalProcessorTime.TotalSeconds - $taskCpuBefore) / $taskClock.Elapsed.TotalSeconds / [Environment]::ProcessorCount
    meanWorkingSetBytes=($taskSamples | Measure-Object workingSetBytes -Average).Average
    meanPrivateBytes=($taskSamples | Measure-Object privateBytes -Average).Average
    scope='This process only; measure Roblox and temporary WebView2 processes separately'
}
$taskReport | ConvertTo-Json | Set-Content -LiteralPath $Output -Encoding utf8
$taskReport
