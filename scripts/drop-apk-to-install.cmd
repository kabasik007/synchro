@echo off
setlocal EnableExtensions
if "%~1"=="" (
  echo Drag an APK file onto this .cmd file to install it on an ADB-authorized Android device.
  echo Or run: drop-apk-to-install.cmd "C:\path\app-debug.apk"
  pause
  exit /b 2
)
if /I not "%~x1"==".apk" (
  echo Not an APK: %~1
  pause
  exit /b 2
)
where adb >nul 2>&1
if errorlevel 1 (
  set "ADB=%LOCALAPPDATA%\Android\Sdk\platform-tools\adb.exe"
) else (
  set "ADB=adb"
)
if not exist "%ADB%" if /I not "%ADB%"=="adb" (
  echo ADB not found. Install Android SDK Platform Tools or add adb.exe to PATH.
  pause
  exit /b 2
)
echo ADB devices:
"%ADB%" devices
echo.
echo Installing: %~nx1
"%ADB%" install -r "%~f1"
if errorlevel 1 (
  echo Installation failed. Check USB or wireless debugging and the authorized device list.
  pause
  exit /b 1
)
echo Installed.
pause
