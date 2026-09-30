param([string]$PackageRoot='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $PackageRoot) { $PackageRoot=Join-Path $projectRoot 'dist/ptools' }
$PackageRoot=(Resolve-Path -LiteralPath $PackageRoot).Path
$runRoot=Join-Path $projectRoot ('artifacts/gesture-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
$dataRoot=Join-Path $runRoot 'data'
New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
$results=[ordered]@{timestamp=(Get-Date).ToString('o');checks=@();input='SendInput Win + left mouse; private fixture';clipboard='Materialized format copies restored in finally'}
function Check([bool]$Ok,[string]$Name) {
    $script:results.checks+=@{name=$Name;passed=$Ok}
    if(-not $Ok) { throw "FAILED: $Name" }; Write-Output "PASS: $Name"
}
function Pump([int]$Milliseconds=60) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do { [Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 10 } while($watch.ElapsedMilliseconds -lt $Milliseconds)
}
function Wait-For([scriptblock]$Condition,[int]$Timeout=8000) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while(-not (& $Condition)) { if($watch.ElapsedMilliseconds -ge $Timeout) { throw "Timeout: $Condition" }; Pump 20 }
}
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class GestureWin {
  public delegate bool EnumProc(IntPtr h,IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p,IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder s,int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h,int id);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
  [DllImport("user32.dll",EntryPoint="SendMessageW")] public static extern IntPtr Send(IntPtr h,uint msg,IntPtr w,IntPtr l);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h,uint msg,IntPtr w,string l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int n);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern uint SendInput(uint n,INPUT[] p,int size);
  [DllImport("ole32.dll")] public static extern int OleInitialize(IntPtr p);
  [DllImport("ole32.dll")] public static extern void OleUninitialize();
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSE { public int X,Y; public uint Data,Flags,Time; public UIntPtr Extra; }
  [StructLayout(LayoutKind.Sequential)] public struct KEY { public ushort Vk,Scan; public uint Flags,Time; public UIntPtr Extra; }
  [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public MOUSE Mouse; [FieldOffset(0)] public KEY Key; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint Type; public UNION Data; }
  public static void Key(int key,bool up) {
    var i=new INPUT { Type=1, Data=new UNION { Key=new KEY { Vk=(ushort)key,Flags=up?2u:0u } } };
    if(SendInput(1,new[]{i},Marshal.SizeOf<INPUT>())!=1) throw new Exception("Key SendInput failed");
  }
  public static void Mouse(int x,int y,uint flags) {
    var i=new INPUT { Data=new UNION { Mouse=new MOUSE { Flags=flags } } };
    if((flags&1)!=0) { i.Data.Mouse.Flags|=0xC000; i.Data.Mouse.X=(int)((long)(x-GetSystemMetrics(76))*65535/(GetSystemMetrics(78)-1)); i.Data.Mouse.Y=(int)((long)(y-GetSystemMetrics(77))*65535/(GetSystemMetrics(79)-1)); }
    if(SendInput(1,new[]{i},Marshal.SizeOf<INPUT>())!=1) throw new Exception("Mouse SendInput failed");
  }
  public static IntPtr[] Windows(uint pid,string match,bool visible) {
    var a=new List<IntPtr>(); EnumWindows((h,l)=>{ uint found; GetWindowThreadProcessId(h,out found); var s=new StringBuilder(256); GetClassName(h,s,256);
      if(found==pid&&(match==""||s.ToString()==match)&&(!visible||IsWindowVisible(h))) a.Add(h); return true; },IntPtr.Zero); return a.ToArray();
  }
}
'@
if([Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') { throw 'Run with pwsh -STA -File scripts/verify-gesture.ps1' }
[void][GestureWin]::OleInitialize([IntPtr]::Zero)
$clipboard=[Windows.Forms.DataObject]::new()
$clipboardCopies=[Collections.Generic.List[IDisposable]]::new()
try {
    $source=[Windows.Forms.Clipboard]::GetDataObject()
    if($source) {
        foreach($format in $source.GetFormats($false)) {
            $value=$source.GetData($format,$false)
            if($value -is [Drawing.Image]) { $value=$value.Clone(); $clipboardCopies.Add($value) }
            elseif($value -is [IO.Stream]) {
                $stream=[IO.MemoryStream]::new(); $position=$value.Position
                $value.Position=0; $value.CopyTo($stream); $value.Position=$position; $stream.Position=0
                $value=$stream; $clipboardCopies.Add($value)
            } elseif($value -is [Array]) { $value=$value.Clone() }
            elseif($null -ne $value -and $value -isnot [string]) { throw "Unsupported clipboard format: $format; no input performed" }
            if($null -eq $value) { throw "Cannot materialize clipboard format: $format; no input performed" }
            $clipboard.SetData($format,$false,$value)
        }
    }
    $source=$null
} catch {
    foreach($copy in $clipboardCopies) { $copy.Dispose() }
    [GestureWin]::OleUninitialize(); throw
}
$foreground=[GestureWin]::GetForegroundWindow()
$cursor=[GestureWin+POINT]::new(); [void][GestureWin]::GetCursorPos([ref]$cursor)
$dpi=[GestureWin]::SetThreadDpiAwarenessContext([IntPtr](-4))
$form=[Windows.Forms.Form]::new(); $form.Text='ptools gesture verification fixture'
$form.StartPosition='Manual'; $form.Location=[Drawing.Point]::new(90,90); $form.ClientSize=[Drawing.Size]::new(760,460)
$form.BackColor=[Drawing.Color]::FromArgb(246,247,249)
$script:mouseDowns=0; $script:mouseUps=0
$form.add_MouseDown({$script:mouseDowns++}); $form.add_MouseUp({$script:mouseUps++})
$owned=[Collections.Generic.List[Diagnostics.Process]]::new()
function Start-Host {
    $settings=@{hotkey='Ctrl+Alt+Shift+Space';plugin_hotkeys=@{'capture:capture'='';'capture:pin'='';'capture:history'='';'capture:custom'=''}}
    if($script:disableCapture) { $settings.disabled_plugins=@('capture') }
    $settings | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $dataRoot 'settings.json') -Encoding utf8NoBOM
    $info=[Diagnostics.ProcessStartInfo]::new((Join-Path $PackageRoot 'ptools.exe'))
    $info.UseShellExecute=$false; $info.CreateNoWindow=$true
    foreach($arg in @('--hidden','--data-dir',$dataRoot)) { $info.ArgumentList.Add($arg) }
    $script:gui=[Diagnostics.Process]::Start($info); $owned.Add($gui)
    Wait-For { $script:hostWindow=@([GestureWin]::Windows($gui.Id,'',$false) | Where-Object { [GestureWin]::GetDlgItem($_,201) -ne [IntPtr]::Zero })[0]; $null -ne $hostWindow }
    Wait-For { Test-Path -LiteralPath (Join-Path $dataRoot 'index.json') }; Pump 400
}
function Begin-Drag([int]$X=220,[int]$Y=210,[int]$Win=0x5B) {
    [void][GestureWin]::SetForegroundWindow($form.Handle); Pump
    [GestureWin]::Mouse($X,$Y,1); Pump 30
    [GestureWin]::Key($Win,$false); Pump 30
    [GestureWin]::Mouse(0,0,2); Pump 30
}
function End-Drag([int]$Win=0x5B) { [GestureWin]::Mouse(0,0,4); Pump 30; [GestureWin]::Key($Win,$true); Pump 180 }
function Count-Images {
    $path=Join-Path $dataRoot 'plugin-data/capture/images/index.json'
    if(Test-Path -LiteralPath $path) { @(Get-Content -LiteralPath $path -Raw | ConvertFrom-Json).Count } else { 0 }
}
function Close-Pins {
    Wait-For { $script:record=Get-CimInstance Win32_Process -Filter "ParentProcessId=$($gui.Id) AND Name='ptools-capture.exe'"; $null -ne $record }
    $script:child=[Diagnostics.Process]::GetProcessById($record.ProcessId); $owned.Add($child)
    Wait-For { @([GestureWin]::Windows($child.Id,'ptools.capture.view',$true)).Count -gt 0 }
    foreach($h in [GestureWin]::Windows($child.Id,'ptools.capture.view',$true)) { [void][GestureWin]::PostMessage($h,0x0010,[IntPtr]::Zero,[IntPtr]::Zero) }
    Check ($child.WaitForExit(5000)) 'Closing quick pins releases the capture process'
}
try {
    $form.Show(); Pump 150; Start-Host
    $gui.Refresh(); $results.idle_before=@{working_set_mib=[Math]::Round($gui.WorkingSet64/1MB,2);private_commit_mib=[Math]::Round($gui.PrivateMemorySize64/1MB,2);threads=$gui.Threads.Count;handles=$gui.HandleCount}
    Check (@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($gui.Id) AND Name='ptools-capture.exe'").Count -eq 0) 'Gesture listener does not start a resident capture process'
    Begin-Drag
    [GestureWin]::Mouse(460,330,1); Pump 100
    $actual=[GestureWin+POINT]::new(); [void][GestureWin]::GetCursorPos([ref]$actual)
    Check ([Math]::Abs($actual.X-460) -le 1 -and [Math]::Abs($actual.Y-330) -le 1) 'Cursor follows the real drag'
    Check (@([GestureWin]::Windows($gui.Id,'ptools.capture.outline',$true)).Count -eq 1) 'Only the transient selection border appears while dragging'
    $watch=[Diagnostics.Stopwatch]::StartNew(); End-Drag
    Wait-For { (Count-Images) -eq 1 }
    Wait-For { $script:record=Get-CimInstance Win32_Process -Filter "ParentProcessId=$($gui.Id) AND Name='ptools-capture.exe'"; $null -ne $record }
    Wait-For { $script:pin=@([GestureWin]::Windows($record.ProcessId,'ptools.capture.view',$true))[0]; $null -ne $pin }
    $results.release_to_pin_ms=[Math]::Round($watch.Elapsed.TotalMilliseconds,2)
    $r=[GestureWin+RECT]::new()
    Wait-For { [void][GestureWin]::GetWindowRect($pin,[ref]$r); $r.Left -eq 220 -and $r.Top -eq 210 } 2000
    $results.first_pin=@{left=$r.Left;top=$r.Top;width=$r.Right-$r.Left;height=$r.Bottom-$r.Top}
    Check ($r.Left -eq 220 -and $r.Top -eq 210 -and $r.Right-$r.Left -eq 240 -and $r.Bottom-$r.Top -eq 120) 'Quick screenshot pins at the selected physical rectangle'
    $item=@(Get-Content -LiteralPath (Join-Path $dataRoot 'plugin-data/capture/images/index.json') -Raw | ConvertFrom-Json)[0]
    Check ($item.width -eq 240 -and $item.height -eq 120 -and $item.source -eq '截图') 'One screenshot enters the persistent shared history'
    $image=[Drawing.Bitmap]::new((Join-Path $dataRoot "plugin-data/capture/images/$($item.id).png"))
    $saved=$image.GetPixel(0,0); $image.Dispose()
    $results.saved_corner=@{r=$saved.R;g=$saved.G;b=$saved.B}
    Check ($saved.R -eq 246 -and $saved.G -eq 247 -and $saved.B -eq 249) 'Selection border is excluded from the saved screenshot'
    $copied=[Windows.Forms.Clipboard]::GetImage()
    Check ($null -ne $copied -and $copied.Width -eq 240 -and $copied.Height -eq 120) 'Quick screenshot is also copied to the clipboard'
    if($copied) { $copied.Dispose() }
    [uint32]$frontPid=0; [void][GestureWin]::GetWindowThreadProcessId([GestureWin]::GetForegroundWindow(),[ref]$frontPid)
    Check ((Get-Process -Id $frontPid).ProcessName -ne 'StartMenuExperienceHost') 'Releasing Win after capture does not open the Start menu'
    Check ($mouseDowns -eq 0 -and $mouseUps -eq 0) 'Captured gesture does not click or drag the underlying application'
    Close-Pins
    Begin-Drag 500 350 0x5C; [GestureWin]::Mouse(280,230,1); Pump; End-Drag 0x5C
    Wait-For { (Count-Images) -eq 2 }
    Check ($true) 'Right Win and reverse-direction drag both capture'
    Close-Pins
    Begin-Drag; [GestureWin]::Mouse(440,320,1); Pump
    [GestureWin]::Key(0x1B,$false); [GestureWin]::Key(0x1B,$true); Pump; End-Drag
    Check ((Count-Images) -eq 2 -and @([GestureWin]::Windows($gui.Id,'ptools.capture.outline',$true)).Count -eq 0) 'Esc cancels without leaving a screenshot or border'
    Begin-Drag; [GestureWin]::Mouse(450,320,1); Pump
    [GestureWin]::Key(0x5B,$true); Pump; [GestureWin]::Mouse(0,0,4); Pump 200
    Check ((Count-Images) -eq 2) 'Releasing Win before the mouse cancels capture'
    Begin-Drag; End-Drag
    Check ((Count-Images) -eq 2) 'Win click without a rectangle creates no screenshot'
    [void][GestureWin]::SetForegroundWindow($form.Handle); Pump
    [GestureWin]::Mouse(240,230,1); [GestureWin]::Mouse(0,0,2); [GestureWin]::Mouse(0,0,4); Pump
    Check ($mouseDowns -eq 1 -and $mouseUps -eq 1) 'Ordinary mouse clicks still reach the underlying application'
    Check ([GestureWin]::GetAsyncKeyState(0x5B) -ge 0 -and [GestureWin]::GetAsyncKeyState(0x5C) -ge 0) 'Win keys are released normally without a sticky modifier'
    $gui.Refresh(); $cpu=$gui.TotalProcessorTime.TotalMilliseconds; Pump 2000; $gui.Refresh()
    $results.idle_after=@{working_set_mib=[Math]::Round($gui.WorkingSet64/1MB,2);private_commit_mib=[Math]::Round($gui.PrivateMemorySize64/1MB,2);threads=$gui.Threads.Count;handles=$gui.HandleCount;cpu_ms=$gui.TotalProcessorTime.TotalMilliseconds-$cpu}
    Check (@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($gui.Id) AND Name='ptools-capture.exe'").Count -eq 0) 'Completed and cancelled gestures leave no resident tool process'
    [void][GestureWin]::PostMessage($hostWindow,0x8001,[IntPtr]::Zero,[IntPtr]::Zero); Pump 100
    [void][GestureWin]::SendMessage([GestureWin]::GetDlgItem($hostWindow,201),0x000C,[IntPtr]::Zero,'快速截图')
    [void][GestureWin]::PostMessage($hostWindow,0x8004,[IntPtr]0x0D,[IntPtr]::Zero)
    Wait-For { $script:record=Get-CimInstance Win32_Process -Filter "ParentProcessId=$($gui.Id) AND Name='ptools-capture.exe'"; $null -ne $record }
    Wait-For { $script:selection=@([GestureWin]::Windows($record.ProcessId,'ptools.capture.view',$true))[0]; $null -ne $selection }
    $origin=[GestureWin+RECT]::new(); [void][GestureWin]::GetWindowRect($selection,[ref]$origin)
    $start=(220-$origin.Left) -bor ((210-$origin.Top) -shl 16)
    $end=(460-$origin.Left) -bor ((330-$origin.Top) -shl 16)
    [void][GestureWin]::Send($selection,0x0201,[IntPtr]1,[IntPtr]$start)
    [void][GestureWin]::Send($selection,0x0200,[IntPtr]1,[IntPtr]$end)
    [void][GestureWin]::Send($selection,0x0202,[IntPtr]::Zero,[IntPtr]$end)
    Wait-For { (Count-Images) -eq 3 }
    Check ($true) 'Search entry opens quick selection and mouse release directly pins it'
    Close-Pins
    $gui.Kill(); $gui.WaitForExit(); $script:disableCapture=$true; Start-Host
    Begin-Drag; [GestureWin]::Mouse(420,300,1); Pump; End-Drag
    Check ((Count-Images) -eq 3 -and $mouseDowns -eq 2 -and $mouseUps -eq 2) 'Disabled capture plugin removes the Win-drag interception'
    $results.status='passed'
} finally {
    [GestureWin]::Mouse(0,0,4)
    foreach($key in @(0x5B,0x5C,0x1B)) { [GestureWin]::Key($key,$true) }
    foreach($process in $owned) { try { if(-not $process.HasExited) { $process.Kill(); $process.WaitForExit() } } catch {} }
    $form.Close(); $form.Dispose()
    try {
        [Windows.Forms.Clipboard]::SetDataObject($clipboard,$true,20,50)
        $results.clipboard_restored=$true
    } catch { $results.clipboard_restored=$false; $results.clipboard_error=$_.Exception.Message }
    foreach($copy in $clipboardCopies) { $copy.Dispose() }
    [GestureWin]::OleUninitialize()
    [void][GestureWin]::SetCursorPos($cursor.X,$cursor.Y); [void][GestureWin]::SetForegroundWindow($foreground)
    [void][GestureWin]::SetThreadDpiAwarenessContext($dpi)
    $results.checks+=@{name='Clipboard format copies restored after input tests';passed=$results.clipboard_restored}
    if(-not $results.clipboard_restored) { $results.status='failed' }
    $results | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $runRoot 'results.json') -Encoding utf8NoBOM
    Write-Output "Evidence: $runRoot"
    if(-not $results.clipboard_restored) { throw 'Clipboard restoration failed; see results.json' }
}
