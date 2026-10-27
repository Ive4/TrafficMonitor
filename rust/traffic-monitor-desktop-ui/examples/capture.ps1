param([string]$OutputDirectory = "$PSScriptRoot/../../../target/desktop-ui-captures")
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DesktopCapture {
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c,string t);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p,IntPtr after,string c,string t);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h,uint command);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 public struct Rect {public int L,T,R,B;}
}
"@
[void][DesktopCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$main=[IntPtr]::Zero
for($attempt=0;$attempt -lt 100 -and $main -eq [IntPtr]::Zero;$attempt++){
 $main=[DesktopCapture]::FindWindow('TrafficMonitor.Rust.Desktop','TrafficMonitor isolated UI fixture')
 if($main -eq [IntPtr]::Zero){Start-Sleep -Milliseconds 50}
}
if ($main -eq [IntPtr]::Zero) {throw 'Start desktop_demo with a sufficient --seconds timeout first.'}
$ownerPid=[uint32]0
[void][DesktopCapture]::GetWindowThreadProcessId($main,[ref]$ownerPid)
# Wait for the first sampled dashboard and the compositor to present the child.
Start-Sleep -Milliseconds 1500
function Capture-Window([IntPtr]$Handle,[string]$Name){
 $rect=New-Object DesktopCapture+Rect
 [void][DesktopCapture]::GetWindowRect($Handle,[ref]$rect)
 $bitmap=[System.Drawing.Bitmap]::new($rect.R-$rect.L,$rect.B-$rect.T)
 $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
 $hdc=$graphics.GetHdc()
 try {[void][DesktopCapture]::PrintWindow($Handle,$hdc,2)} finally {$graphics.ReleaseHdc($hdc)}
 try {$bitmap.Save((Join-Path $OutputDirectory "$Name.png"),[System.Drawing.Imaging.ImageFormat]::Png)} finally {$graphics.Dispose();$bitmap.Dispose()}
}
Capture-Window $main 'floating'
$taskbar=[DesktopCapture]::FindWindow('Shell_TrayWnd',$null)
$child=[DesktopCapture]::FindWindowEx($taskbar,[IntPtr]::Zero,'TrafficMonitor.Rust.Desktop','TrafficMonitor taskbar')
if($child -ne [IntPtr]::Zero){
 $childPid=[uint32]0
 [void][DesktopCapture]::GetWindowThreadProcessId($child,[ref]$childPid)
 if($childPid -ne $ownerPid){throw 'Refusing to capture a taskbar child owned by another process.'}
 Capture-Window $child 'taskbar'
 $rect=New-Object DesktopCapture+Rect
 [void][DesktopCapture]::GetWindowRect($child,[ref]$rect)
 $bitmap=[System.Drawing.Bitmap]::new($rect.R-$rect.L,$rect.B-$rect.T)
 $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
 try {
  $graphics.CopyFromScreen($rect.L,$rect.T,0,0,$bitmap.Size)
  $bitmap.Save((Join-Path $OutputDirectory 'taskbar-visible.png'),[System.Drawing.Imaging.ImageFormat]::Png)
  $colors=[System.Collections.Generic.HashSet[int]]::new()
  for($y=0;$y -lt $bitmap.Height;$y++){for($x=0;$x -lt $bitmap.Width;$x++){[void]$colors.Add($bitmap.GetPixel($x,$y).ToArgb())}}
  [pscustomobject]@{OwnerPid=$ownerPid;TaskbarChildPid=$childPid;X=$rect.L;Y=$rect.T;Width=$bitmap.Width;Height=$bitmap.Height;DistinctColors=$colors.Count}|ConvertTo-Json|Set-Content (Join-Path $OutputDirectory 'taskbar-visible.json')
  if($colors.Count -lt 3){throw 'Taskbar child visible capture contains no rendered text/graph pixels.'}
 } finally {$graphics.Dispose();$bitmap.Dispose()}
}
foreach($item in @(@(100,'Settings'),@(114,'Taskbar appearance'),@(101,'Traffic history'),@(102,'Network connections'),@(103,'Skins and language'),@(104,'Plugin manager'))){
 [void][DesktopCapture]::PostMessage($main,273,[IntPtr]$item[0],[IntPtr]::Zero)
 Start-Sleep -Milliseconds 350
 $panel=[DesktopCapture]::FindWindow('TrafficMonitor.Rust.Desktop',"TrafficMonitor — $($item[1])")
 if($panel -eq [IntPtr]::Zero -or [DesktopCapture]::GetWindow($panel,4) -ne $main){throw "Missing fixture-owned panel: $($item[1])"}
 Capture-Window $panel $item[0]
}
