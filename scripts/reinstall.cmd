@echo off
taskkill /im phrasex-app.exe /f
timeout /t 2 /nobreak >nul
"C:\Users\Marco\Desktop\PhraseX\target\release\bundle\nsis\Phrase X_0.1.0_x64-setup.exe" /S
echo setup exit %errorlevel% > "%~dp0reinstall.log"
ie4uinit.exe -ClearIconCache
ie4uinit.exe -show
start "" "%LOCALAPPDATA%\Phrase X\phrasex-app.exe"
