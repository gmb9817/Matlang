param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = "Stop"

if (-not $CargoArgs -or $CargoArgs.Count -eq 0) {
    $CargoArgs = @("test", "--workspace")
}

function Find-VsDevCmd {
    $vswhere = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $installPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($LASTEXITCODE -eq 0 -and $installPath) {
            $candidate = Join-Path $installPath "Common7\Tools\VsDevCmd.bat"
            if (Test-Path $candidate) {
                return $candidate
            }
        }
    }

    $candidates = @(
        "C:\Program Files\Microsoft Visual Studio\18\Community\Common7\Tools\VsDevCmd.bat",
        "C:\Program Files\Microsoft Visual Studio\18\BuildTools\Common7\Tools\VsDevCmd.bat",
        "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat",
        "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat"
    )

    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            return $candidate
        }
    }

    throw "Unable to locate VsDevCmd.bat. Install Visual Studio C++ build tools or update scripts/cargo-msvc.ps1."
}

function Find-Cargo {
    $cargo = Get-Command cargo.exe -ErrorAction SilentlyContinue
    if ($cargo) {
        return $cargo.Source
    }

    $fallback = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path $fallback) {
        return $fallback
    }

    throw "Unable to locate cargo.exe. Install Rust or update scripts/cargo-msvc.ps1."
}

$vsDevCmd = Find-VsDevCmd
$cargoExe = Find-Cargo
$repoRoot = Split-Path -Parent $PSScriptRoot
$joinedCargoArgs = ($CargoArgs | ForEach-Object {
        if ($_ -match '[\s"]') {
            '"' + $_.Replace('"', '\"') + '"'
        } else {
            $_
        }
    }) -join " "

$command = "call `"$vsDevCmd`" -arch=x64 >nul && cd /d `"$repoRoot`" && `"$cargoExe`" $joinedCargoArgs"
& cmd.exe /c $command
exit $LASTEXITCODE
