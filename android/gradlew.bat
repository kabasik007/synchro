@echo off
rem Synchro Gradle 8.13 bootstrap for Windows.
setlocal
set "APP_HOME=%~dp0"
set "WRAPPER_JAR=%APP_HOME%gradle\wrapper\gradle-wrapper.jar"
if not exist "%WRAPPER_JAR%" (
  powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%APP_HOME%scripts\bootstrap-wrapper.ps1"
  if errorlevel 1 exit /b 1
)
if defined JAVA_HOME (
  set "JAVA_EXE=%JAVA_HOME%\bin\java.exe"
) else (
  set "JAVA_EXE=java.exe"
)
if not exist "%JAVA_EXE%" (
  if not "%JAVA_EXE%"=="java.exe" (
    echo JAVA_HOME is invalid. Use JDK 17 or configure Android Studio Gradle JDK to 17.
    exit /b 1
  )
)
"%JAVA_EXE%" -Xms64m -Xmx64m -classpath "%WRAPPER_JAR%" org.gradle.wrapper.GradleWrapperMain %*
exit /b %ERRORLEVEL%
