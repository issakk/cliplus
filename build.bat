@echo off
REM ClipSync 构建脚本
REM 自动查找并设置 MSVC 环境，然后执行 cargo build

REM 查找 vswhere.exe
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (
    echo [ERROR] 未找到 Visual Studio Installer
    exit /b 1
)

REM 查找最新 VS 安装路径
for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -property installationPath`) do set "VS_PATH=%%i"
if not defined VS_PATH (
    echo [ERROR] 未找到 Visual Studio 安装
    exit /b 1
)

REM 设置 MSVC 环境
call "%VS_PATH%\VC\Auxiliary\Build\vcvarsall.bat" x64
if errorlevel 1 (
    echo [ERROR] vcvarsall.bat 执行失败
    exit /b 1
)

REM 构建
cd /d "%~dp0src-tauri"
echo.
echo [INFO] 开始构建 ClipSync...
cargo build %*
