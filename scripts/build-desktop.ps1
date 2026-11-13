param([string]$OutputDirectory = 'target\desktop', [switch]$Smoke, [switch]$Debug)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    . "$PSScriptRoot\environment.ps1"

    function Resolve-GpuiFxcCompiler {
        param([string]$RootDir)
        if ($env:GPUI_FXC_PATH -and (Test-Path -LiteralPath $env:GPUI_FXC_PATH)) {
            return $env:GPUI_FXC_PATH
        }
        $cmd = Get-Command fxc.exe -ErrorAction SilentlyContinue
        if ($cmd -and (Test-Path -LiteralPath $cmd.Source)) {
            return $cmd.Source
        }
        $sdk = Get-ChildItem -Path 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\fxc.exe' -ErrorAction SilentlyContinue |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($sdk) {
            return $sdk.FullName
        }
        $gcc = Get-Command gcc.exe -ErrorAction SilentlyContinue
        if ($gcc) {
            $toolsDir = Join-Path $RootDir 'target\tools'
            New-Item -ItemType Directory -Force -Path $toolsDir | Out-Null
            $cPath = Join-Path $toolsDir 'fxc-d3dcompiler.c'
            $exePath = Join-Path $toolsDir 'fxc-d3dcompiler.exe'
            if (-not (Test-Path -LiteralPath $exePath)) {
                $cSource = @'
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <d3dcompiler.h>
#include <stdio.h>
#include <stdlib.h>

static int same_ci(const char *a, const char *b) { return _stricmp(a, b) == 0; }

int main(int argc, char **argv) {
    const char *target = NULL, *entry = NULL, *out_header = NULL, *var_name = "g_main", *input_path = NULL;
    UINT flags = D3DCOMPILE_OPTIMIZATION_LEVEL1;
    for (int i = 1; i < argc; i++) {
        const char *arg = argv[i];
        if (same_ci(arg, "/T") && i + 1 < argc) target = argv[++i];
        else if (same_ci(arg, "/E") && i + 1 < argc) entry = argv[++i];
        else if (same_ci(arg, "/Fh") && i + 1 < argc) out_header = argv[++i];
        else if (same_ci(arg, "/Vn") && i + 1 < argc) var_name = argv[++i];
        else if (same_ci(arg, "/O0")) flags = D3DCOMPILE_OPTIMIZATION_LEVEL0;
        else if (same_ci(arg, "/O1")) flags = D3DCOMPILE_OPTIMIZATION_LEVEL1;
        else if (same_ci(arg, "/O2")) flags = D3DCOMPILE_OPTIMIZATION_LEVEL2;
        else if (same_ci(arg, "/O3")) flags = D3DCOMPILE_OPTIMIZATION_LEVEL3;
        else if (arg[0] != '/') input_path = arg;
    }
    if (!target || !entry || !out_header || !input_path) return 1;
    int wlen = MultiByteToWideChar(CP_UTF8, 0, input_path, -1, NULL, 0);
    if (wlen <= 0) return 1;
    WCHAR *winput = (WCHAR *)malloc((size_t)wlen * sizeof(WCHAR));
    if (!winput) return 1;
    MultiByteToWideChar(CP_UTF8, 0, input_path, -1, winput, wlen);
    ID3DBlob *code = NULL, *errors = NULL;
    HRESULT hr = D3DCompileFromFile(winput, NULL, D3D_COMPILE_STANDARD_FILE_INCLUDE, entry, target, flags, 0, &code, &errors);
    free(winput);
    if (errors) {
        fwrite(errors->lpVtbl->GetBufferPointer(errors), 1, errors->lpVtbl->GetBufferSize(errors), stderr);
        errors->lpVtbl->Release(errors);
    }
    if (FAILED(hr) || !code) return 1;
    const unsigned char *bytes = (const unsigned char *)code->lpVtbl->GetBufferPointer(code);
    SIZE_T size = code->lpVtbl->GetBufferSize(code);
    FILE *fp = fopen(out_header, "wb");
    if (!fp) { code->lpVtbl->Release(code); return 1; }
    fprintf(fp, "const BYTE %s[] =\n{\n", var_name);
    for (SIZE_T i = 0; i < size; i++) {
        if (i % 12 == 0) fprintf(fp, "    ");
        fprintf(fp, "0x%02X%s", bytes[i], (i + 1 < size) ? ", " : "");
        if (i % 12 == 11 || i + 1 == size) fprintf(fp, "\n");
    }
    fprintf(fp, "};\n");
    fclose(fp);
    code->lpVtbl->Release(code);
    return 0;
}
'@
                [System.IO.File]::WriteAllText($cPath, $cSource, [System.Text.UTF8Encoding]::new($false))
                & $gcc.Source -O2 $cPath -o $exePath -ld3dcompiler
                if ($LASTEXITCODE -ne 0) { throw 'Failed to build fxc-d3dcompiler helper' }
            }
            return $exePath
        }
        throw 'Cannot build release GPUI shaders: set GPUI_FXC_PATH or install Windows 10/11 SDK (fxc.exe).'
    }

    $taskProfile = if ($Debug -or $OutputDirectory -match '(^|[\\/])debug([\\/]|$)') { 'debug' } else { 'release' }
    $taskArgs = @('build', '-p', 'rolauncher-desktop', '--locked')
    if ($taskProfile -eq 'release') {
        $env:GPUI_FXC_PATH = Resolve-GpuiFxcCompiler -RootDir $taskRoot
        $taskArgs += '--release'
    }
    if ($Smoke) {
        $taskArgs += '--features', 'ui-smoke'
    }

    & cargo @taskArgs
    if ($LASTEXITCODE -ne 0) { throw 'GPUI desktop build failed' }

    $taskBuiltExe = Join-Path $taskRoot "target\$taskProfile\rolauncher-desktop.exe"
    if (-not (Test-Path -LiteralPath $taskBuiltExe)) { throw "Built GPUI desktop executable missing: $taskBuiltExe" }

    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
    Copy-Item -LiteralPath $taskBuiltExe -Destination (Join-Path $OutputDirectory 'RoLauncher.Desktop.exe') -Force
    if (Test-Path -LiteralPath 'assets\RoLauncher.ico') {
        Copy-Item -LiteralPath 'assets\RoLauncher.ico' -Destination (Join-Path $OutputDirectory 'RoLauncher.ico') -Force
    }
    if (Test-Path -LiteralPath 'desktop\CHANGELOG.txt') {
        Copy-Item -LiteralPath 'desktop\CHANGELOG.txt' -Destination (Join-Path $OutputDirectory 'CHANGELOG.txt') -Force
    }

    $taskLicenses = Join-Path $OutputDirectory 'licenses'
    New-Item -ItemType Directory -Force -Path $taskLicenses | Out-Null
    $taskHost = (& rustc -vV | Select-String '^host: ').ToString().Substring(6).Trim()
    $taskMetadata = & cargo metadata --locked --offline --filter-platform $taskHost --format-version 1 | ConvertFrom-Json
    $taskById = @{}
    foreach ($p in $taskMetadata.packages) { $taskById[$p.id] = $p }
    $taskNodes = @{}
    foreach ($n in $taskMetadata.resolve.nodes) { $taskNodes[$n.id] = $n }
    $taskRootPkg = $taskMetadata.packages | Where-Object { $_.name -eq 'rolauncher-desktop' } | Select-Object -First 1
    $taskSeen = [System.Collections.Generic.HashSet[string]]::new()
    $taskQueue = [System.Collections.Generic.Queue[string]]::new()
    if ($taskRootPkg) { $taskQueue.Enqueue($taskRootPkg.id) }
    while ($taskQueue.Count -gt 0) {
        $cur = $taskQueue.Dequeue()
        if (-not $taskSeen.Add($cur)) { continue }
        $node = $taskNodes[$cur]
        if ($node) {
            foreach ($dep in $node.dependencies) { $taskQueue.Enqueue($dep) }
        }
    }
    $taskRecords = @()
    foreach ($pkgId in ($taskSeen | Sort-Object)) {
        $pkg = $taskById[$pkgId]
        if (-not $pkg -or $pkg.name -eq 'rolauncher' -or $pkg.name -eq 'rolauncher-desktop') { continue }
        $pkgDir = Split-Path -Parent $pkg.manifest_path
        $noticeDir = Join-Path $taskLicenses ($pkg.name + '-' + $pkg.version)
        $files = @(Get-ChildItem -LiteralPath $pkgDir -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^(?i)(license|copying|notice|unlicense|patent)' })
        if ($pkg.license_file) {
            $explicit = Join-Path $pkgDir $pkg.license_file
            if (Test-Path -LiteralPath $explicit) { $files += (Get-Item -LiteralPath $explicit) }
        }
        $copied = @()
        if ($files.Count -gt 0) {
            New-Item -ItemType Directory -Force -Path $noticeDir | Out-Null
            foreach ($f in ($files | Select-Object -Unique)) {
                Copy-Item -LiteralPath $f.FullName -Destination (Join-Path $noticeDir $f.Name) -Force
                $copied += $f.Name
            }
        }
        $taskRecords += [pscustomobject]@{
            name = $pkg.name
            version = $pkg.version
            license = $pkg.license
            repository = $pkg.repository
            notices = $copied
        }
    }
    [System.IO.File]::WriteAllText(
        (Join-Path (Resolve-Path -LiteralPath $taskLicenses).Path 'dependencies.json'),
        ($taskRecords | ConvertTo-Json -Depth 4),
        [System.Text.UTF8Encoding]::new($false)
    )
}
finally {
    Pop-Location
}
