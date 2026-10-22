param([string]$OutPath = "C:\Users\Lab\AppData\Local\Temp\opencode\probe.png")
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[W]::SetProcessDPIAware() | Out-Null
$p = Get-Process -Name mbscada -ErrorAction Stop | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
$r = New-Object W+RECT
$ok = [W]::GetWindowRect($p.MainWindowHandle, [ref]$r)
Write-Output ("rect ok=$ok L={0} T={1} R={2} B={3} W={4} H={5}" -f $r.Left,$r.Top,$r.Right,$r.Bottom,($r.Right-$r.Left),($r.Bottom-$r.Top))
$vs = [System.Windows.Forms.SystemInformation]::VirtualScreen
Write-Output ("virtualscreen L={0} T={1} W={2} H={3}" -f $vs.X,$vs.Y,$vs.Width,$vs.Height)
$ss = [System.Windows.Forms.Screen]::AllScreens
foreach ($s in $ss) { Write-Output ("screen {0} primary={1} bounds={2}" -f $s.DeviceName, $s.Primary, $s.Bounds) }
$bmp = New-Object System.Drawing.Bitmap($vs.Width, $vs.Height)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($vs.X, $vs.Y, 0, 0, $bmp.Size)
$g.Dispose()
$bmp.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "saved $OutPath"
