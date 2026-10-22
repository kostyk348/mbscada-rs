param(
    [int]$X = 0, [int]$Y = 0,
    [string]$Keys = "",
    [switch]$NoClick,
    [string]$ExpectTitle = "mbSCADA",
    [switch]$WaitDialog
)

Add-Type -AssemblyName System.Windows.Forms
$dll = "C:\Users\Lab\AppData\Local\Temp\opencode\uit3.dll"
$src = @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
namespace UiT3 {
  public static class Native {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder sb, int n);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, [Out] int[] r);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, int d, UIntPtr e);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool att);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    public static List<string> Windows = new List<string>();
  }
}
"@
if (Test-Path $dll) {
    Add-Type -Path $dll
} else {
    Add-Type -TypeDefinition $src -OutputAssembly $dll
    Add-Type -Path $dll
}
[UiT3.Native]::SetProcessDPIAware() | Out-Null

$exePid = (Get-Process mbscada -ErrorAction Stop | Select-Object -First 1).Id

# enumerate windows of the mbscada process, remember titles
$cb = [UiT3.Native+EnumProc]{ param($h, $l)
    $pid2 = 0
    [UiT3.Native]::GetWindowThreadProcessId($h, [ref]$pid2) | Out-Null
    if ($pid2 -eq $exePid) {
        $sb = New-Object System.Text.StringBuilder 256
        [UiT3.Native]::GetWindowText($h, $sb, 256) | Out-Null
        $cls = New-Object System.Text.StringBuilder 256
        [UiT3.Native]::GetClassName($h, $cls, 256) | Out-Null
        if ($sb.Length -gt 0 -or $cls.ToString() -eq "#32770") {
            [UiT3.Native]::Windows.Add("$($cls.ToString())|$($sb.ToString())|$h") | Out-Null
        }
    }
    return $true
}
[UiT3.Native]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
[UiT3.Native]::Windows | ForEach-Object { Write-Output "WIN: $_" }

if ($WaitDialog) {
    $dlgs = [UiT3.Native]::Windows | Where-Object { $_ -like "#32770|*" }
    if (-not $dlgs) { Write-Output "NO_DIALOG"; exit 3 }
    Write-Output "DIALOG_FOUND"
    exit 0
}

# resolve target: prefer a top-level titled window matching ExpectTitle
$wins = [UiT3.Native]::Windows | Where-Object { $_ -match [regex]::Escape($ExpectTitle) }
if (-not $wins) { Write-Output "ABORT_WINDOW_NOT_FOUND ('$ExpectTitle')"; exit 2 }
$parts = ($wins | Select-Object -First 1).Split('|')
$h = [IntPtr][long]$parts[2]
$title = $parts[1]

# restore if minimized
if ([UiT3.Native]::IsIconic($h)) {
    [UiT3.Native]::ShowWindow($h, 9) | Out-Null   # SW_RESTORE
    Start-Sleep -Milliseconds 700
}

# verify the target is actually foreground (else our click could hit someone else)
$fg = [UiT3.Native]::GetForegroundWindow()
if ($fg -ne $h) {
    # Windows foreground-lock workaround: common-input-thread attach + ALT nudge + attach
    $fgPid = 0; $fgThread = [UiT3.Native]::GetWindowThreadProcessId($fg, [ref]$fgPid)
    $me = [UiT3.Native]::GetCurrentThreadId()
    [UiT3.Native]::AttachThreadInput($fgThread, $me, $true) | Out-Null
    [UiT3.Native]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)   # ALT down
    [UiT3.Native]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)   # ALT up
    [UiT3.Native]::BringWindowToTop($h) | Out-Null
    [UiT3.Native]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 700
    [UiT3.Native]::AttachThreadInput($fgThread, $me, $false) | Out-Null
    $fg = [UiT3.Native]::GetForegroundWindow()
}
if ($fg -ne $h) { Write-Output "ABORT_FOREGROUND title='$title'"; exit 4 }

$r = [int[]]::new(4)
$rOk = [UiT3.Native]::GetWindowRect($h, $r)
Write-Output ("RECT ok={0} L={1} T={2} R={3} B={4} title='{5}'" -f $rOk, $r[0], $r[1], $r[2], $r[3], $title)
if ($X -ge 0 -and $Y -ge 0 -and -not $NoClick) {
    $sx = $r.L + $X; $sy = $r.T + $Y
    [UiT3.Native]::SetCursorPos($sx, $sy) | Out-Null
    Start-Sleep -Milliseconds 250
    [UiT3.Native]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 70
    [UiT3.Native]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
    Write-Output "clicked ($sx,$sy)"
}
if ($Keys -ne "") {
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
    Start-Sleep -Milliseconds 300
    Write-Output "sent keys"
}
