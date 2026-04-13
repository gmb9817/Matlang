@echo off
setlocal

set "SCRIPT_DIR=%~dp0"
for %%I in ("%SCRIPT_DIR%..") do set "REPO_ROOT=%%~fI"

set "VSDEVCMD="
set "VSWHERE=C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
if exist "%VSWHERE%" (
    for /f "usebackq delims=" %%I in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do (
        set "VSDEVCMD=%%~fI\Common7\Tools\VsDevCmd.bat"
    )
)

if not defined VSDEVCMD if exist "C:\Program Files\Microsoft Visual Studio\18\Community\Common7\Tools\VsDevCmd.bat" set "VSDEVCMD=C:\Program Files\Microsoft Visual Studio\18\Community\Common7\Tools\VsDevCmd.bat"
if not defined VSDEVCMD if exist "C:\Program Files\Microsoft Visual Studio\18\BuildTools\Common7\Tools\VsDevCmd.bat" set "VSDEVCMD=C:\Program Files\Microsoft Visual Studio\18\BuildTools\Common7\Tools\VsDevCmd.bat"
if not defined VSDEVCMD if exist "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat" set "VSDEVCMD=C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat"
if not defined VSDEVCMD if exist "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" set "VSDEVCMD=C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat"

if not defined VSDEVCMD (
    echo Unable to locate VsDevCmd.bat. Install Visual Studio C++ build tools or update scripts\cargo-msvc.cmd.
    exit /b 1
)

set "CARGO_EXE="
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "CARGO_EXE=%USERPROFILE%\.cargo\bin\cargo.exe"
if not defined CARGO_EXE for %%I in (cargo.exe) do set "CARGO_EXE=%%~$PATH:I"

if not defined CARGO_EXE (
    echo Unable to locate cargo.exe. Install Rust or update scripts\cargo-msvc.cmd.
    exit /b 1
)

call "%VSDEVCMD%" -arch=x64 >nul
if errorlevel 1 exit /b %errorlevel%

cd /d "%REPO_ROOT%"
if errorlevel 1 exit /b %errorlevel%

if "%~1"=="" (
    "%CARGO_EXE%" test --workspace
) else (
    "%CARGO_EXE%" %*
)

exit /b %errorlevel%
