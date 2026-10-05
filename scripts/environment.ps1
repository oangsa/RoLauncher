# Workspace-only GNU tools are optional. Normal builds use installed Rust + MSVC.
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskCargo = Join-Path $taskRoot '.tools\cargo\bin'
if (Test-Path -LiteralPath $taskCargo) {
    $env:CARGO_HOME = Join-Path $taskRoot '.tools\cargo'
    $env:RUSTUP_HOME = Join-Path $taskRoot '.tools\rustup'
    $env:PATH = $taskCargo + ';' + $env:PATH
}
$taskCompiler = Join-Path $taskRoot '.tools\gcc\mingw64\bin'
if (Test-Path -LiteralPath $taskCompiler) {
    $env:PATH = $taskCompiler + ';' + $env:PATH
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = 'gcc.exe'
    $env:CC = 'gcc.exe'
}
$taskDotnet = Join-Path $taskRoot '.tools\dotnet'
if (Test-Path -LiteralPath (Join-Path $taskDotnet 'dotnet.exe')) {
    $env:PATH = $taskDotnet + ';' + $env:PATH
    $env:DOTNET_ROOT = $taskDotnet
    $env:DOTNET_CLI_HOME = Join-Path $taskRoot '.tools\dotnet-home'
}
$env:NUGET_PACKAGES = Join-Path $taskRoot '.tools\nuget'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
