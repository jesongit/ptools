param([string]$PackageRoot='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $PackageRoot) { $PackageRoot=Join-Path $projectRoot 'dist/ptools' }
$PackageRoot=(Resolve-Path -LiteralPath $PackageRoot).Path
$runRoot=Join-Path $projectRoot ('artifacts/tools-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
$dataRoot=Join-Path $runRoot 'data'
New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
$results=[ordered]@{timestamp=(Get-Date).ToString('o');checks=@();keyboard_mode='WM_HOTKEY dispatch';network_translation='not tested; no credentials';real_uninstall='not performed'}
function Check([bool]$Ok,[string]$Name) {
    $script:results.checks+=@{name=$Name;passed=$Ok}
    if(-not $Ok) { throw "FAILED: $Name" }; Write-Output "PASS: $Name"
}
function Wait-For([scriptblock]$Condition,[int]$Timeout=10000) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while(-not (& $Condition)) { if($watch.ElapsedMilliseconds -ge $Timeout) { throw "Timeout: $Condition" }; Start-Sleep -Milliseconds 20 }
}
function Start-Tool([string]$Exe,[string[]]$Arguments,[switch]$Interactive) {
    $info=[Diagnostics.ProcessStartInfo]::new($Exe)
    $info.UseShellExecute=$false; $info.CreateNoWindow=$true
    $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true; $info.RedirectStandardInput=$Interactive.IsPresent
    foreach($arg in $Arguments) { $info.ArgumentList.Add($arg) }
    [Diagnostics.Process]::Start($info)
}
function Invoke-Tool([string]$Exe,[string[]]$Arguments) {
    $process=Start-Tool $Exe $Arguments
    $out=$process.StandardOutput.ReadToEnd(); $err=$process.StandardError.ReadToEnd(); $process.WaitForExit()
    if($process.ExitCode -ne 0) { throw "$Exe failed: $err" }
    $out
}
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class ToolsWin {
  public delegate bool EnumProc(IntPtr hwnd, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder text,int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h,int id);
  [DllImport("user32.dll",EntryPoint="SendMessageW")] public static extern IntPtr Send(IntPtr h,uint msg,IntPtr w,IntPtr l);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h,uint msg,IntPtr w,string l);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  public static IntPtr[] Windows(uint pid,string match,bool visible) {
    var result=new List<IntPtr>();
    EnumWindows((h,l)=>{uint found;GetWindowThreadProcessId(h,out found); var text=new StringBuilder(256);GetClassName(h,text,256);
      if(found==pid && (match=="" || text.ToString()==match) && (!visible || IsWindowVisible(h))) result.Add(h);return true;},IntPtr.Zero);
    return result.ToArray();
  }
}
'@
function Snapshot([IntPtr]$Window,[string]$Name) {
    $dpi=[ToolsWin]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $r=[ToolsWin+RECT]::new(); [void][ToolsWin]::GetWindowRect($Window,[ref]$r)
    $bitmap=[Drawing.Bitmap]::new($r.Right-$r.Left,$r.Bottom-$r.Top)
    $graphics=[Drawing.Graphics]::FromImage($bitmap); $dc=$graphics.GetHdc()
    try { [void][ToolsWin]::PrintWindow($Window,$dc,2) } finally { $graphics.ReleaseHdc($dc) }
    $bitmap.Save((Join-Path $runRoot $Name),[Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bitmap.Dispose(); [void][ToolsWin]::SetThreadDpiAwarenessContext($dpi)
}
function Memory([Diagnostics.Process]$Process) {
    $Process.Refresh()
    $value=[ordered]@{working_set_mib=[Math]::Round($Process.WorkingSet64/1MB,2);private_commit_mib=[Math]::Round($Process.PrivateMemorySize64/1MB,2);handles=$Process.HandleCount}
    $counter=Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($Process.Id)" -ErrorAction SilentlyContinue
    if($counter) { $value.private_working_set_mib=[Math]::Round($counter.WorkingSetPrivate/1MB,2) }
    $value
}
function Send-Invocation([Diagnostics.Process]$Process,[string]$Action,[string]$Root) {
    $json=@{protocol_version=2;operation='invoke';action=$Action;data_dir=$Root} | ConvertTo-Json -Compress
    $Process.StandardInput.WriteLine($json); $Process.StandardInput.Flush()
}
$capture=Join-Path $PackageRoot 'plugins/capture/ptools-capture.exe'
$uninstaller=Join-Path $PackageRoot 'plugins/uninstaller/ptools-uninstaller.exe'
$hostExe=Join-Path $PackageRoot 'ptools.exe'
$owned=[Collections.Generic.List[Diagnostics.Process]]::new()
$foreground=[ToolsWin]::GetForegroundWindow()
try {
    $probe=Invoke-Tool $capture @('--probe-ocr') | ConvertFrom-Json
    Check ($probe.recognition.text -match 'Hello' -and $probe.recognition.text -match '12345' -and $probe.recognition.words.Count -gt 0) 'Offline OCR recognizes rendered Chinese/English/numbers and word positions'
    $results.ocr=$probe
    $software=Invoke-Tool $uninstaller @('--list') | ConvertFrom-Json
    Check ($software.Count -gt 0) 'Desktop uninstall registry enumeration'
    $results.desktop_software_count=$software.Count
    # Create only a private history fixture; no clipboard or real application changes.
    $captureRoot=Join-Path $dataRoot 'plugin-data/capture'
    $images=Join-Path $captureRoot 'images'; New-Item -ItemType Directory -Path $images -Force | Out-Null
    $bitmap=[Drawing.Bitmap]::new(800,400); $graphics=[Drawing.Graphics]::FromImage($bitmap)
    $graphics.Clear([Drawing.Color]::White)
    $font=[Drawing.Font]::new('Microsoft YaHei',24)
    $graphics.DrawString('历史与贴图测试 Hello ptools 12345',$font,[Drawing.Brushes]::Black,30,30)
    $graphics.DrawRectangle([Drawing.Pens]::RoyalBlue,24,24,750,350)
    $bitmap.Save((Join-Path $images '1-1.png'),[Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Save((Join-Path $images '1-1-thumb.png'),[Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $font.Dispose(); $bitmap.Dispose()
    @(@{id='1-1';created=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds();source='验收图片';width=800;height=400}) | ConvertTo-Json -AsArray | Set-Content -LiteralPath (Join-Path $images 'index.json') -Encoding utf8NoBOM
    $settings=@{hotkey='Ctrl+Alt+Space';plugin_hotkeys=@{'capture:capture'='Ctrl+Alt+Shift+1';'capture:pin'='Ctrl+Alt+Shift+2';'capture:history'='Ctrl+Alt+Shift+3';'capture:custom'='Ctrl+Alt+Shift+9'}}
    $settings | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $dataRoot 'settings.json') -Encoding utf8NoBOM
    $gui=Start-Tool $hostExe @('--hidden','--data-dir',$dataRoot); $owned.Add($gui)
    Wait-For { $script:hostWindow=@([ToolsWin]::Windows($gui.Id,'', $false) | Where-Object { [ToolsWin]::GetDlgItem($_,201) -ne [IntPtr]::Zero })[0]; $hostWindow -ne $null }
    Wait-For { Test-Path -LiteralPath (Join-Path $dataRoot 'index.json') }
    Start-Sleep -Milliseconds 400
    Check (@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($gui.Id) AND (Name='ptools-capture.exe' OR Name='ptools-uninstaller.exe')").Count -eq 0) 'Interactive tools never run during host indexing'
    $results.host_before_tools=Memory $gui
    $watch=[Diagnostics.Stopwatch]::StartNew()
    [void][ToolsWin]::PostMessage($hostWindow,0x0312,[IntPtr]1000,[IntPtr]::Zero)
    Wait-For { $script:record=Get-CimInstance Win32_Process -Filter "ParentProcessId=$($gui.Id) AND Name='ptools-capture.exe'"; $null -ne $record }
    $child=[Diagnostics.Process]::GetProcessById($record.ProcessId); $owned.Add($child)
    Wait-For { $script:overlay=@([ToolsWin]::Windows($child.Id,'ptools.capture.view',$true))[0]; $overlay -ne $null }
    $results.capture_cold_visible_ms=[Math]::Round($watch.Elapsed.TotalMilliseconds,2)
    # Select 600x300; annotations are drawn into this native surface.
    [void][ToolsWin]::Send($overlay,0x0201,[IntPtr]1,[IntPtr](100 -bor (100 -shl 16)))
    [void][ToolsWin]::Send($overlay,0x0200,[IntPtr]1,[IntPtr](700 -bor (400 -shl 16)))
    [void][ToolsWin]::Send($overlay,0x0202,[IntPtr]::Zero,[IntPtr](700 -bor (400 -shl 16)))
    Snapshot $overlay 'capture.png'
    $results.capture_active=Memory $child
    [void][ToolsWin]::PostMessage($overlay,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    Check ($child.WaitForExit(5000)) 'Last capture window closes and process exits'
    # Invoke history through the same host; double-click opens a pin in the same child.
    [void][ToolsWin]::PostMessage($hostWindow,0x0312,[IntPtr]1002,[IntPtr]::Zero)
    Wait-For { $script:record=Get-CimInstance Win32_Process -Filter "ParentProcessId=$($gui.Id) AND Name='ptools-capture.exe'"; $null -ne $record }
    $child=[Diagnostics.Process]::GetProcessById($record.ProcessId); $owned.Add($child)
    Wait-For { $script:history=@([ToolsWin]::Windows($child.Id,'ptools.capture.view',$true))[0]; $history -ne $null }
    Snapshot $history 'history.png'
    Check ([ToolsWin]::Send([ToolsWin]::GetDlgItem($history,1),0x018B,[IntPtr]::Zero,[IntPtr]::Zero).ToInt64() -eq 1) 'Persistent unified image history loads'
    [void][ToolsWin]::Send($history,0x0111,[IntPtr](1 -bor (2 -shl 16)),[IntPtr]::Zero)
    Wait-For { @([ToolsWin]::Windows($child.Id,'ptools.capture.view',$true)).Count -eq 2 }
    $pin=@([ToolsWin]::Windows($child.Id,'ptools.capture.view',$true) | Where-Object { $_ -ne $history })[0]
    Snapshot $pin 'pin.png'
    [void][ToolsWin]::PostMessage($hostWindow,0x0312,[IntPtr]1002,[IntPtr]::Zero)
    Wait-For { @([ToolsWin]::Windows($child.Id,'ptools.capture.view',$true)).Count -eq 3 }
    Check (@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($gui.Id) AND Name='ptools-capture.exe'").Count -eq 1) 'Multiple actions and pins reuse one process'
    foreach($h in [ToolsWin]::Windows($child.Id,'ptools.capture.view',$true)) { [void][ToolsWin]::PostMessage($h,0x0010,[IntPtr]::Zero,[IntPtr]::Zero) }
    Check ($child.WaitForExit(5000)) 'Closing all history and pin windows leaves no resident child'
    # Settings use ordinary native fields; cancel avoids changing credentials or configuration.
    $tool=Start-Tool $capture @('--interactive') -Interactive; $owned.Add($tool)
    Send-Invocation $tool 'settings' $captureRoot
    Wait-For { $script:form=@([ToolsWin]::Windows($tool.Id,'ptools.tool.form',$true))[0]; $form -ne $null }
    Snapshot $form 'capture-settings.png'
    Check ([ToolsWin]::GetDlgItem($form,10) -ne [IntPtr]::Zero) 'Native settings fields render'
    [void][ToolsWin]::PostMessage($form,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    Check ($tool.WaitForExit(5000)) 'Cancel settings releases the tool process'
    $tool=Start-Tool $uninstaller @('--interactive') -Interactive; $owned.Add($tool)
    Send-Invocation $tool 'open' (Join-Path $dataRoot 'plugin-data/uninstaller')
    Wait-For { $script:uninstallWindow=@([ToolsWin]::Windows($tool.Id,'ptools.uninstaller',$true))[0]; $uninstallWindow -ne $null }
    $list=[ToolsWin]::GetDlgItem($uninstallWindow,2)
    Check ([ToolsWin]::Send($list,0x1004,[IntPtr]::Zero,[IntPtr]::Zero).ToInt64() -gt 0) 'Desktop software list renders'
    Snapshot $uninstallWindow 'uninstaller.png'
    $results.uninstaller_active=Memory $tool
    [void][ToolsWin]::SendMessage([ToolsWin]::GetDlgItem($uninstallWindow,1),0x000C,[IntPtr]::Zero,'zz_ptools_no_such_software')
    Check ([ToolsWin]::Send($list,0x1004,[IntPtr]::Zero,[IntPtr]::Zero).ToInt64() -eq 0) 'Software filtering responds without filesystem scanning'
    [void][ToolsWin]::PostMessage($uninstallWindow,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    Check ($tool.WaitForExit(5000)) 'Uninstaller closes without a resident process'
    $results.host_after_tools=Memory $gui
    $gui.Refresh(); $cpu=$gui.TotalProcessorTime.TotalMilliseconds
    Start-Sleep -Seconds 2; $gui.Refresh()
    $results.host_idle_cpu_ms=$gui.TotalProcessorTime.TotalMilliseconds-$cpu
    $results.file_sizes=@(Get-Item -LiteralPath $hostExe,$capture,$uninstaller | ForEach-Object { @{name=$_.Name;bytes=$_.Length} })
    # Reopen a capture, terminate only our test host, and confirm Job Object cleanup.
    [void][ToolsWin]::PostMessage($hostWindow,0x0312,[IntPtr]1000,[IntPtr]::Zero)
    Wait-For { $script:record=Get-CimInstance Win32_Process -Filter "ParentProcessId=$($gui.Id) AND Name='ptools-capture.exe'"; $null -ne $record }
    $child=[Diagnostics.Process]::GetProcessById($record.ProcessId); $owned.Add($child)
    $gui.Kill(); $gui.WaitForExit()
    Check ($child.WaitForExit(5000)) 'Terminating host reclaims the interactive process tree'
    $results.status='passed'
} finally {
    foreach($process in $owned) { if(-not $process.HasExited) { $process.Kill(); $process.WaitForExit() } }
    [void][ToolsWin]::SetForegroundWindow($foreground)
    $results | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $runRoot 'results.json') -Encoding utf8NoBOM
    Write-Output "Evidence: $runRoot"
}
