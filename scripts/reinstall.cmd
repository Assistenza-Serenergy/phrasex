@echo off
rem Run through scripts\reinstall.vbs (hidden window). The app may run as administrator,
rem so instead of killing it we ask it to quit (it polls for this file) and wait.
echo.> "%APPDATA%\PhraseX\quit-request"
taskkill /im phrasex-app.exe /f >nul 2>&1
set n=0
:wait
tasklist /fi "imagename eq phrasex-app.exe" | find /i "phrasex-app.exe" >nul || goto install
set /a n+=1
if %n% geq 20 goto install
timeout /t 1 /nobreak >nul
goto wait
:install
del "%APPDATA%\PhraseX\quit-request" >nul 2>&1
rem newest installer, whatever the version
set "nsis=%~dp0..\target\release\bundle\nsis"
for /f "delims=" %%f in ('dir /b /o:d "%nsis%\*-setup.exe"') do set "setup=%nsis%\%%f"
"%setup%" /S
echo setup exit %errorlevel% > "%~dp0reinstall.log"
ie4uinit.exe -ClearIconCache
ie4uinit.exe -show
start "" "%LOCALAPPDATA%\Phrase X\phrasex-app.exe"
