param(
    [ValidateSet('x64','x86')][string]$Architecture='x64',
    [string]$Executable,
    [ValidateRange(0,8191)][int]$DisplayMask=15,
    [string]$Language='en-US',
    [int]$TaskbarOffset=0,
    [switch]$TaskbarOnly
)
$ErrorActionPreference='Stop'
$repoRoot=Split-Path -Parent $PSScriptRoot
if(-not $Executable){$Executable=Join-Path $repoRoot "Bin/Rust/$Architecture/TrafficMonitor.exe"}
$executable=(Resolve-Path -LiteralPath $Executable).Path
$runDirectory=Join-Path $repoRoot "target/native-desktop-captures/$Architecture-$([DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff'))"
New-Item -ItemType Directory -Force $runDirectory | Out-Null
[IO.File]::WriteAllText((Join-Path $runDirectory 'config.ini'),"[general]`nlanguage=$Language`ncheck_update_when_start=false`n[config]`nshow_cpu_memory=true`n[task_bar]`ntbar_display_item=$DisplayMask`nwindow_offset_left=$TaskbarOffset`n")
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class NativeDesktopCapture {
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c,string t);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p,IntPtr after,string c,string t);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h,uint command);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint process);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder name,int capacity);
 public delegate bool Visit(IntPtr h,IntPtr parameter);
 [DllImport("user32.dll")] public static extern bool EnumWindows(Visit visitor,IntPtr parameter);
 public static IntPtr FindOwnedPanel(IntPtr owner,int process) {
  IntPtr result=IntPtr.Zero;
  EnumWindows((h,p)=>{uint pid;GetWindowThreadProcessId(h,out pid);var name=new StringBuilder(256);GetClassName(h,name,256);if(pid==process&&GetWindow(h,4)==owner&&name.ToString()=="TrafficMonitor.Rust.Desktop"){result=h;return false;}return true;},IntPtr.Zero);
  return result;
 }
 public static IntPtr PrimaryTaskbar(){return FindWindow("Shell_TrayWnd",null);}
 public static IntPtr FindOwnedMain(int process) {
  IntPtr result=IntPtr.Zero;
  EnumWindows((h,p)=>{uint pid;GetWindowThreadProcessId(h,out pid);var name=new StringBuilder(256);GetClassName(h,name,256);if(pid==process&&GetWindow(h,4)==IntPtr.Zero&&name.ToString()=="TrafficMonitor.Rust.Desktop"){result=h;return false;}return true;},IntPtr.Zero);return result;
 }
 public static IntPtr FindOwnedChild(IntPtr parent,int process,string classname,string title) {
  IntPtr child=IntPtr.Zero;while((child=FindWindowEx(parent,child,classname,title))!=IntPtr.Zero){uint pid;GetWindowThreadProcessId(child,out pid);if(pid==process)return child;}return IntPtr.Zero;
 }
 public struct Rect {public int L,T,R,B;}
}
'@
function Capture([IntPtr]$Window,[string]$Name,[switch]$Screen) {
    [uint32]$ownerProcess=0
    [void][NativeDesktopCapture]::GetWindowThreadProcessId($Window,[ref]$ownerProcess)
    if ($ownerProcess -ne $process.Id) {throw 'Refusing to capture a window outside the owned validation process'}
    $rect=[NativeDesktopCapture+Rect]::new()
    if (-not [NativeDesktopCapture]::GetWindowRect($Window,[ref]$rect)) {throw 'Window disappeared'}
    $bitmap=[Drawing.Bitmap]::new($rect.R-$rect.L,$rect.B-$rect.T)
    $graphics=[Drawing.Graphics]::FromImage($bitmap)
    if($Screen){$graphics.CopyFromScreen($rect.L,$rect.T,0,0,$bitmap.Size)}else{
      $dc=$graphics.GetHdc()
      try {if (-not [NativeDesktopCapture]::PrintWindow($Window,$dc,2)) {throw "PrintWindow failed: $Name"}} finally {$graphics.ReleaseHdc($dc)}
    }
    try {$bitmap.Save((Join-Path $runDirectory "$Name.png"),[Drawing.Imaging.ImageFormat]::Png)} finally {$graphics.Dispose();$bitmap.Dispose()}
}
$info=[Diagnostics.ProcessStartInfo]::new($executable)
$info.UseShellExecute=$false
foreach($value in @('--data-dir',$runDirectory,'--no-plugins','--taskbar','--exit-after-ms','15000')) {$info.ArgumentList.Add($value)}
$process=[Diagnostics.Process]::Start($info)
try {
    $main=[IntPtr]::Zero
    for($attempt=0;$attempt -lt 100 -and $main -eq [IntPtr]::Zero;$attempt++) {
        $main=[NativeDesktopCapture]::FindOwnedMain($process.Id)
        if($main -eq [IntPtr]::Zero){Start-Sleep -Milliseconds 50}
    }
    if($main -eq [IntPtr]::Zero){throw 'Rust main window was not created'}
    Start-Sleep -Milliseconds 1200
    Capture $main 'floating'
    $taskbar=[NativeDesktopCapture]::PrimaryTaskbar()
    $child=[NativeDesktopCapture]::FindOwnedChild($taskbar,$process.Id,'TrafficMonitor.Rust.Desktop','TrafficMonitor taskbar')
    if(($DisplayMask -eq 0) -ne ($child -eq [IntPtr]::Zero)){throw "Taskbar visibility does not match selected display items: $DisplayMask"}
    if($child -ne [IntPtr]::Zero){Capture $child 'taskbar';Capture $child 'taskbar-screen' -Screen}
    $pages=if($TaskbarOnly){@()}else{@(@(100,'Settings'),@(114,'Taskbar appearance'),@(101,'Traffic history'),@(102,'Network connections'),@(103,'Skins and language'),@(104,'Plugin manager'))}
    foreach($item in $pages) {
        [void][NativeDesktopCapture]::PostMessage($main,273,[IntPtr]$item[0],[IntPtr]::Zero)
        Start-Sleep -Milliseconds 450
        $panel=[NativeDesktopCapture]::FindOwnedPanel($main,$process.Id)
        if($panel -eq [IntPtr]::Zero -or [NativeDesktopCapture]::GetWindow($panel,4) -ne $main){throw "Owned panel not found: $($item[1])"}
        Capture $panel ([string]$item[0])
        if($item[0] -eq 100){
          $tabs=[NativeDesktopCapture]::FindOwnedChild($panel,$process.Id,'SysTabControl32',$null)
          if($tabs -eq [IntPtr]::Zero){throw 'Settings does not contain real tabs'}
          $count=[NativeDesktopCapture]::SendMessage($tabs,0x1304,[IntPtr]::Zero,[IntPtr]::Zero).ToInt32()
          if($count -ne 7){throw "Expected seven settings tabs, found $count"}
          for($index=0;$index -lt $count;$index++){
            $selected=[NativeDesktopCapture]::SendMessage($tabs,0x130b,[IntPtr]::Zero,[IntPtr]::Zero).ToInt32()
            if($selected -ne $index){throw "Unexpected settings tab index: $selected / $index"}
            Capture $panel "settings-tab-$index"
            if($index+1 -lt $count){[void][NativeDesktopCapture]::PostMessage($tabs,0x100,[IntPtr]0x27,[IntPtr]::Zero);[void][NativeDesktopCapture]::PostMessage($tabs,0x101,[IntPtr]0x27,[IntPtr]::Zero);Start-Sleep -Milliseconds 250}
          }
        }
    }
    [void][NativeDesktopCapture]::PostMessage($main,16,[IntPtr]::Zero,[IntPtr]::Zero)
    if(-not $process.WaitForExit(20000)){$process.Kill();throw 'Owned validation process timed out'}
    if($process.ExitCode -ne 0){throw "Validation process failed: $($process.ExitCode)"}
    Get-ChildItem -LiteralPath $runDirectory -Filter '*.png' | Select-Object Name,Length,FullName
}
finally {if(-not $process.HasExited){[void][NativeDesktopCapture]::PostMessage($main,16,[IntPtr]::Zero,[IntPtr]::Zero);if(-not $process.WaitForExit(5000)){$process.Kill()}}}
