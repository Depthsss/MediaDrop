@echo off
chcp 65001 >nul
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0release-mediadrop.ps1"
set "EXIT_CODE=%ERRORLEVEL%"
echo.
if not "%EXIT_CODE%"=="0" echo Release basarisiz oldu. Hata kodu: %EXIT_CODE%
if "%EXIT_CODE%"=="0" echo Release islemi tamamlandi.
echo Kapatmak icin bir tusa basin.
>nul 2>&1 set /p "="
exit /b %EXIT_CODE%
