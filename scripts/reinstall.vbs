' Runs reinstall.cmd with no console window: explorer.exe scripts\reinstall.vbs
Set sh = CreateObject("WScript.Shell")
sh.Run "cmd /c """ & Replace(WScript.ScriptFullName, ".vbs", ".cmd") & """", 0, False
