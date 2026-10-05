$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    . "$PSScriptRoot\environment.ps1"
    & cargo test --locked
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
    & cargo fmt --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed' }
    & dotnet restore desktop.tests/RoLauncher.Desktop.Tests.csproj --locked-mode --configfile desktop/NuGet.Config
    if ($LASTEXITCODE -ne 0) { throw 'Desktop test restore failed' }
    & dotnet run --project desktop.tests/RoLauncher.Desktop.Tests.csproj --no-restore
    if ($LASTEXITCODE -ne 0) { throw 'Desktop checks failed' }
    & "$PSScriptRoot\build-desktop.ps1" -Smoke -OutputDirectory 'target\ui-smoke'
    & python "$PSScriptRoot\ui-smoke.py"
    if ($LASTEXITCODE -ne 0) { throw 'WinUI smoke test failed' }
    $env:ROLAUNCHER_DESKTOP_TEST_EXE = Join-Path $taskRoot 'target\ui-smoke\RoLauncher.Desktop.exe'
    $env:ROLAUNCHER_UI_SMOKE_DIR = Join-Path $taskRoot 'target\ui-bridge'
    $env:ROLAUNCHER_UI_BRIDGE_SMOKE = '1'
    & cargo test --locked desktop::tests::winui_bootstrap_pipe_connects_to_real_rust_api_and_exits -- --ignored
    if ($LASTEXITCODE -ne 0) { throw 'Rust/WinUI bootstrap test failed' }
    Remove-Item Env:ROLAUNCHER_DESKTOP_TEST_EXE,Env:ROLAUNCHER_UI_SMOKE_DIR,Env:ROLAUNCHER_UI_BRIDGE_SMOKE
} finally { Pop-Location }
