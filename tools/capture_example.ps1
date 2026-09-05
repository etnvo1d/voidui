param(
    [string]$Example = 'counter',
    [string]$Out = '',
    [string]$Arch = 'x64',
    [ValidateSet('debug', 'release', 'releasedbg')]
    [string]$Mode = 'debug'
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class WinMove {
  [DllImport("user32.dll")] public static extern bool SetWindowPos(
      IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
'@

$root = Join-Path $PSScriptRoot '..'
$exe = Join-Path $root ('build/windows/{0}/{1}/examples/{2}.exe' -f $Arch, $Mode, $Example)
$err = Join-Path $root ('build/{0}.stderr.txt' -f $Example)
if ($Out -eq '') { $Out = Join-Path $root ('build/capture-{0}.png' -f $Example) }

if (-not (Test-Path $exe)) { throw "no such example: $exe" }

$proc = Start-Process -FilePath $exe -PassThru -RedirectStandardError $err

try {
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 100
        $proc.Refresh()
        if ($proc.HasExited) { throw "$Example.exe exited early with code $($proc.ExitCode)" }
        if ($proc.MainWindowHandle -ne [IntPtr]::Zero) { break }
    }
    if ($proc.MainWindowHandle -eq [IntPtr]::Zero) { throw 'no main window appeared' }

    # The OS cascades new windows toward the bottom-right, where a large one
    # runs off screen and the grab would miss part of the render.
    [void][WinMove]::SetWindowPos($proc.MainWindowHandle, [IntPtr]::Zero, 0, 0, 0, 0, 0x0005)
    [void][WinMove]::SetForegroundWindow($proc.MainWindowHandle)
    Start-Sleep -Milliseconds 800

    # Grab the whole virtual screen. Locating the client area by its clear colour
    # downstream is more reliable than ClientToScreen, which does not account for
    # per-monitor DPI scaling here.
    $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap($b.Width, $b.Height)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.X, $b.Y, 0, 0, (New-Object System.Drawing.Size($b.Width, $b.Height)))
    $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()

    Write-Output "captured $($b.Width)x$($b.Height) -> $Out"
}
finally {
    if (-not $proc.HasExited) { $proc.Kill(); $proc.WaitForExit(3000) }
}
