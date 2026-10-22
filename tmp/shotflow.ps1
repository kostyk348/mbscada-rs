# Screenshot flow: launch app, click "Импорт папки...", type Desktop path in native
# dialog, Enter, click "Старт", capture docs/img/app.png.
# Safety: every click goes through ui3.ps1 which ABORTS (exit 4) if the target
# window is not in the foreground.
$ErrorActionPreference = "Stop"
$root = "C:\Users\Lab\Documents\mbscada-rs"
$exe  = "$root\target\x86_64-pc-windows-gnu\release\mbscada.exe"
$ui3  = "$root\tmp\ui3.ps1"

function Ui3 {
    $out = & powershell -ExecutionPolicy Bypass -File $ui3 @args 2>&1
    $out | ForEach-Object { Write-Output "    $_" }
    if ($LASTEXITCODE -ne 0) { throw "ui3 failed code=$LASTEXITCODE" }
}

# 0. launch app if not running
if (-not (Get-Process mbscada -ErrorAction SilentlyContinue)) {
    Start-Process $exe | Out-Null
    Start-Sleep 6
}
if (-not (Get-Process mbscada -ErrorAction SilentlyContinue)) { throw "app died on launch" }

# 1. focus main window (no click)
Ui3 -NoClick

# 2. click "Импорт папки..." (window-relative)
Ui3 -X 1286 -Y 83

# 3. wait for native dialog (owned by mbscada)
Start-Sleep 2
$p = Start-Process powershell -ArgumentList "-ExecutionPolicy","Bypass","-File",$ui3,"-WaitDialog" -Wait -PassThru -NoNewWindow -RedirectStandardOutput "C:\Users\Lab\AppData\Local\Temp\opencode\dlg.out" -RedirectStandardError "C:\Users\Lab\AppData\Local\Temp\opencode\dlg.err"
Get-Content "C:\Users\Lab\AppData\Local\Temp\opencode\dlg.out" -ErrorAction SilentlyContinue | ForEach-Object { Write-Output "    $_" }
if ($p.ExitCode -ne 0) { throw "dialog wait failed code=$($p.ExitCode)" }

# 4. type folder path (clipboard paste, Cyrillic-safe) + Enter
Set-Clipboard -Value "C:\Users\Lab\Desktop"
Ui3 -NoClick -ExpectTitle Modbus -Keys "^a^v{ENTER}"

# 5. imports may take a moment
Start-Sleep 6
if (-not (Get-Process mbscada -ErrorAction SilentlyContinue)) { throw "app died after import" }

# 6. click Старт
Ui3 -X 1432 -Y 83
Start-Sleep 4
if (-not (Get-Process mbscada -ErrorAction SilentlyContinue)) { throw "app died after start" }

# 7. screenshot
& powershell -ExecutionPolicy Bypass -File "$root\tmp\screenshot.ps1" -OutPath "$root\docs\img\app.png"
Write-Output "SHOT_DONE"
