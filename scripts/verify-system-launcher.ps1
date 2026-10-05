param([string]$Executable='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $Executable) { $Executable=Join-Path $projectRoot 'dist/ptools/ptools.exe' }
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$runRoot=Join-Path $projectRoot ('artifacts/system-launcher-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8))
$dataRoot=Join-Path $runRoot 'data'
New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
# The isolated host has no user usage history, and does not register other plugins' hotkeys.
@{hotkey='Ctrl+Alt+Space';disabled_plugins=@('capture','uninstaller');usage=@{};plugin_hotkeys=@{}} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $dataRoot 'settings.json') -Encoding utf8NoBOM
$results=[ordered]@{timestamp=(Get-Date).ToString('o');executable=$Executable;data_dir=$dataRoot;status='running';checks=@();skipped=@();launched=@();cleanup=@();input='WM_HOTKEY 101, WM_SETTEXT, result selection and Enter through the launcher'}
$ownedWindows=[Collections.Generic.List[object]]::new()
$gui=$null; $shell=$null; $originalForeground=[IntPtr]::Zero

function Check([bool]$Ok,[string]$Name,[object]$Evidence=$null) {
    $script:results.checks += [ordered]@{name=$Name;passed=$Ok;evidence=$Evidence}
    if(-not $Ok) { throw "FAILED: $Name" }
    Write-Output "PASS: $Name"
}
function Skip([string]$Name,[string]$Reason) {
    $script:results.skipped += [ordered]@{name=$Name;reason=$Reason}
    Write-Output "SKIP: $Name ($Reason)"
}
function Wait-For([scriptblock]$Condition,[string]$Name,[int]$TimeoutMs=10000) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while($watch.ElapsedMilliseconds -lt $TimeoutMs) {
        if(& $Condition) { return }
        Start-Sleep -Milliseconds 50
    }
    throw "Timeout after $TimeoutMs ms: $Name"
}

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public sealed class SystemLauncherWindow {
  public long Handle;
  public uint ProcessId;
  public string Title;
  public string ClassName;
  public bool Visible;
}
public static class SystemLauncherWin {
  public delegate bool EnumProc(IntPtr h,IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,StringBuilder text,int count);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder text,int count);
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h,int id);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW",SetLastError=true)] private static extern IntPtr SendRaw(IntPtr h,uint msg,IntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW",CharSet=CharSet.Unicode,SetLastError=true)] private static extern IntPtr SendText(IntPtr h,uint msg,IntPtr w,string l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW",CharSet=CharSet.Unicode,SetLastError=true)] private static extern IntPtr SendBuffer(IntPtr h,uint msg,IntPtr w,StringBuilder l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT rect);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint from,uint to,bool attach);
  [DllImport("shell32.dll",CharSet=CharSet.Unicode)] private static extern int SHGetKnownFolderPath(ref Guid id,uint flags,IntPtr token,out IntPtr path);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  public static uint ProcessId(IntPtr h) { uint pid; GetWindowThreadProcessId(h,out pid); return pid; }
  public static SystemLauncherWindow[] Windows() {
    var result=new List<SystemLauncherWindow>();
    EnumWindows((h,l)=>{
      var title=new StringBuilder(1024); var cls=new StringBuilder(256);
      GetWindowText(h,title,title.Capacity); GetClassName(h,cls,cls.Capacity);
      result.Add(new SystemLauncherWindow {Handle=h.ToInt64(),ProcessId=ProcessId(h),Title=title.ToString(),ClassName=cls.ToString(),Visible=IsWindowVisible(h)});
      return true;
    },IntPtr.Zero);
    return result.ToArray();
  }
  public static IntPtr FindHost(uint pid) {
    foreach(var item in Windows()) {
      var h=new IntPtr(item.Handle);
      if(item.ProcessId==pid && GetDlgItem(h,201)!=IntPtr.Zero) return h;
    }
    return IntPtr.Zero;
  }
  public static long Send(IntPtr h,uint msg,long w,long l) {
    UIntPtr result;
    if(SendRaw(h,msg,new IntPtr(w),new IntPtr(l),2,1000,out result)==IntPtr.Zero) throw new Exception("Window message failed or timed out: "+msg);
    return unchecked((long)result.ToUInt64());
  }
  public static void SetText(IntPtr h,string text) {
    UIntPtr result;
    if(SendText(h,0x000C,IntPtr.Zero,text,2,1000,out result)==IntPtr.Zero) throw new Exception("WM_SETTEXT failed or timed out");
  }
  public static string[] ListTitles(IntPtr list) {
    long count=Send(list,0x018B,0,0);
    if(count<0 || count>10000) throw new Exception("Invalid launcher list count");
    var titles=new List<string>();
    for(int i=0;i<count;i++) {
      long length=Send(list,0x018A,i,0);
      if(length<0 || length>32760) throw new Exception("Invalid launcher list title");
      var title=new StringBuilder((int)length+1); UIntPtr result;
      if(SendBuffer(list,0x0189,new IntPtr(i),title,2,1000,out result)==IntPtr.Zero) throw new Exception("LB_GETTEXT failed or timed out");
      titles.Add(title.ToString());
    }
    return titles.ToArray();
  }
  public static string DownloadsPath() {
    var id=new Guid("374DE290-123F-4565-9164-39C4925E467B"); IntPtr path;
    int code=SHGetKnownFolderPath(ref id,0,IntPtr.Zero,out path);
    if(code!=0) Marshal.ThrowExceptionForHR(code);
    try { return Marshal.PtrToStringUni(path); } finally { Marshal.FreeCoTaskMem(path); }
  }
  public static bool Focus(IntPtr target) {
    if(target==IntPtr.Zero || !IsWindow(target)) return false;
    uint current=GetCurrentThreadId(), foreground=GetWindowThreadProcessId(GetForegroundWindow(),out uint first), destination=GetWindowThreadProcessId(target,out uint second);
    bool a=current!=foreground && AttachThreadInput(current,foreground,true);
    bool b=current!=destination && destination!=foreground && AttachThreadInput(current,destination,true);
    try { return SetForegroundWindow(target); }
    finally { if(b) AttachThreadInput(current,destination,false); if(a) AttachThreadInput(current,foreground,false); }
  }
}
'@

function Capture-Launcher([string]$Name) {
    $previousDpi=[SystemLauncherWin]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $bitmap=$null; $graphics=$null
    try {
        $rect=[SystemLauncherWin+RECT]::new()
        if(-not [SystemLauncherWin]::GetWindowRect($script:hostWindow,[ref]$rect)) { throw 'Cannot get launcher window rectangle' }
        $bitmap=[Drawing.Bitmap]::new($rect.Right-$rect.Left,$rect.Bottom-$rect.Top)
        $graphics=[Drawing.Graphics]::FromImage($bitmap); $dc=$graphics.GetHdc()
        try { $printed=[SystemLauncherWin]::PrintWindow($script:hostWindow,$dc,2) } finally { $graphics.ReleaseHdc($dc) }
        if(-not $printed) { throw 'Cannot capture launcher window' }
        $bitmap.Save((Join-Path $runRoot $Name),[Drawing.Imaging.ImageFormat]::Png)
    } finally {
        if($graphics) { $graphics.Dispose() }; if($bitmap) { $bitmap.Dispose() }
        [void][SystemLauncherWin]::SetThreadDpiAwarenessContext($previousDpi)
    }
}
function Select-Target([string]$Query,[string]$Title,[string]$ImageName) {
    if(-not [SystemLauncherWin]::IsWindowVisible($script:hostWindow)) {
        if(-not [SystemLauncherWin]::PostMessage($script:hostWindow,0x0312,[IntPtr]101,[IntPtr]::Zero)) { throw 'Cannot dispatch launcher hotkey' }
        Wait-For { [SystemLauncherWin]::IsWindowVisible($script:hostWindow) } 'launcher visible after WM_HOTKEY'
    }
    [SystemLauncherWin]::SetText($script:edit,$Query)
    $titles=@([SystemLauncherWin]::ListTitles($script:list)); $selected=[Array]::IndexOf($titles,$Title)
    Check ($selected -ge 0) "Launcher query selects $Title" ([ordered]@{query=$Query;rows=$titles;selected=$selected})
    [void][SystemLauncherWin]::Send($script:list,0x0186,$selected,0)
    Capture-Launcher $ImageName
}
function Enter-Target {
    if(-not [SystemLauncherWin]::PostMessage($script:edit,0x0100,[IntPtr]0x0D,[IntPtr]::Zero)) { throw 'Cannot send Enter to launcher' }
    Wait-For { -not [SystemLauncherWin]::IsWindowVisible($script:hostWindow) } 'launcher hides after target Enter'
}
function Remember-Window([object]$Window,[string]$Target,[object]$Evidence=$null) {
    if($script:baselineHandles.Contains([long]$Window.Handle)) { throw "Refusing to own an existing window: $Target" }
    if(@($script:ownedWindows | Where-Object { $_.handle -eq $Window.Handle }).Count -eq 0) {
        $record=[ordered]@{target=$Target;handle=[long]$Window.Handle;process_id=[uint32]$Window.ProcessId;title=$Window.Title;class=$Window.ClassName;evidence=$Evidence}
        $script:ownedWindows.Add($record); $script:results.launched += $record
    }
}
function Get-MmcProcesses {
    @(Get-CimInstance Win32_Process -Filter "Name='mmc.exe'" -OperationTimeoutSec 2)
}
function Normalize-Folder([string]$Path) {
    [IO.Path]::GetFullPath($Path).TrimEnd('\','/')
}
function Get-ExplorerWindows {
    $collection=$null
    try {
        $collection=$script:shell.Windows()
        foreach($item in $collection) {
            try {
                $location=[string]$item.LocationURL; $handle=[long]$item.HWND
                $uri=$null; $path=$null
                if([Uri]::TryCreate($location,[UriKind]::Absolute,[ref]$uri) -and $uri.IsFile) { $path=Normalize-Folder $uri.LocalPath }
                $record=@([SystemLauncherWin]::Windows() | Where-Object { $_.Handle -eq $handle }) | Select-Object -First 1
                if($record -and $path) { [pscustomobject]@{Handle=$handle;ProcessId=$record.ProcessId;Title=$record.Title;ClassName=$record.ClassName;Visible=$record.Visible;LocationURL=$location;Path=$path} }
            } catch {
                # Other Explorer tabs may disappear while the Shell collection is enumerated.
            } finally {
                if($item -and [Runtime.InteropServices.Marshal]::IsComObject($item)) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($item) }
            }
        }
    } finally {
        if($collection -and [Runtime.InteropServices.Marshal]::IsComObject($collection)) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($collection) }
    }
}

try {
    $originalForeground=[SystemLauncherWin]::GetForegroundWindow()
    $baselineWindows=@([SystemLauncherWin]::Windows())
    $script:baselineHandles=[Collections.Generic.HashSet[long]]::new()
    foreach($item in $baselineWindows) { [void]$script:baselineHandles.Add($item.Handle) }
    $shell=New-Object -ComObject Shell.Application
    $downloads=Normalize-Folder ([SystemLauncherWin]::DownloadsPath())
    $results.downloads_known_folder=$downloads
    $hostArguments=@('--hidden','--data-dir',('"' + $dataRoot + '"'))
    $gui=Start-Process -FilePath $Executable -ArgumentList $hostArguments -WindowStyle Hidden -PassThru
    $results.host_process_id=$gui.Id
    Wait-For { $script:hostWindow=[SystemLauncherWin]::FindHost($gui.Id); $script:hostWindow -ne [IntPtr]::Zero } 'isolated launcher window with search control 201'
    $script:edit=[SystemLauncherWin]::GetDlgItem($hostWindow,201); $script:list=[SystemLauncherWin]::GetDlgItem($hostWindow,202)
    Check ($list -ne [IntPtr]::Zero) 'Launcher result list exists'
    Wait-For {
        try {
            $cache=Get-Content -LiteralPath (Join-Path $dataRoot 'index.json') -Raw -ErrorAction Stop | ConvertFrom-Json
            @($cache.plugins.applications | Where-Object { $_.target.kind -eq 'system' -and $_.target.id -in @('device-manager','display-settings','downloads') }).Count -eq 3
        } catch { $false }
    } 'application plugin indexes system targets'
    Check (-not [SystemLauncherWin]::IsWindowVisible($hostWindow)) 'Launcher starts hidden'

    Select-Target 'sbglq' '设备管理器' 'device-manager-launcher.png'
    $existingMmc=@(Get-MmcProcesses)
    if($existingMmc.Count -gt 0 -or @($baselineWindows | Where-Object { $_.Title -match '设备管理器|Device Manager' }).Count -gt 0) {
        Skip 'Device Manager launch' 'An existing MMC process or Device Manager window is present; its window will not be changed or closed'
    } else {
        Enter-Target
        Wait-For {
            $found=$false
            # Windows can elevate MMC, preventing a non-elevated WMI client from reading its command line.
            foreach($process in @(Get-MmcProcesses)) {
                foreach($window in @([SystemLauncherWin]::Windows() | Where-Object {
                    $_.Visible -and $_.ProcessId -eq $process.ProcessId -and -not $script:baselineHandles.Contains($_.Handle) -and (
                        $process.CommandLine -match 'devmgmt\.msc' -or (
                            [string]::IsNullOrEmpty($process.CommandLine) -and $_.Title -match '设备管理器|Device Manager'
                        )
                    )
                })) {
                    Remember-Window $window 'device-manager' ([ordered]@{command_line=$process.CommandLine;command_line_readable=(-not [string]::IsNullOrEmpty($process.CommandLine))})
                    $found=$true
                }
            }
            $found
        } 'new visible mmc.exe Device Manager window'
        Check ($true) 'Device Manager opens through launcher Enter'
    }

    Select-Target 'display' '显示设置' 'display-settings-launcher.png'
    $existingSettings=@(Get-Process -Name SystemSettings -ErrorAction SilentlyContinue)
    $settingsBefore=@([SystemLauncherWin]::Windows() | Where-Object { $_.Title -match '^(设置|Settings)$' -and $_.ClassName -in @('ApplicationFrameWindow','Windows.UI.Core.CoreWindow') })
    if($existingSettings.Count -gt 0 -or $settingsBefore.Count -gt 0) {
        Skip 'Display settings launch' 'An existing Settings process or window is present; its page and window will not be changed or closed'
    } else {
        Enter-Target
        Wait-For {
            $settingsIds=@((Get-Process -Name SystemSettings -ErrorAction SilentlyContinue).Id)
            $frameIds=@((Get-Process -Name ApplicationFrameHost -ErrorAction SilentlyContinue).Id)
            $found=$false
            foreach($window in @([SystemLauncherWin]::Windows() | Where-Object {
                $_.Visible -and -not $script:baselineHandles.Contains($_.Handle) -and (
                    $_.ProcessId -in $settingsIds -or ($_.ProcessId -in $frameIds -and $_.Title -match '^(设置|Settings)$')
                )
            })) {
                Remember-Window $window 'display-settings' ([ordered]@{query='display';settings_processes=$settingsIds})
                $found=$true
            }
            $found
        } 'new visible Windows Settings window'
        Check ($true) 'Display settings target opens a visible Settings window through launcher Enter'
    }

    Select-Target '下载文件夹' '下载文件夹' 'downloads-launcher.png'
    $downloadsBefore=@(Get-ExplorerWindows | Where-Object { $_.Path -eq $downloads })
    if($downloadsBefore.Count -gt 0) {
        Skip 'Downloads folder launch' 'The Downloads folder is already open; its Explorer window will not be changed or closed'
    } else {
        Enter-Target
        Wait-For {
            $found=$false
            foreach($window in @(Get-ExplorerWindows | Where-Object { $_.Visible -and $_.Path -eq $downloads -and $_.ClassName -in @('CabinetWClass','ExploreWClass') -and -not $script:baselineHandles.Contains($_.Handle) })) {
                $process=Get-Process -Id $window.ProcessId -ErrorAction SilentlyContinue
                if($process -and $process.ProcessName -eq 'explorer') {
                    Remember-Window $window 'downloads' ([ordered]@{location_url=$window.LocationURL;resolved_path=$window.Path;known_folder=$downloads})
                    $found=$true
                }
            }
            $found
        } 'new visible Explorer window at the actual Downloads known-folder path'
        Check ($true) 'Downloads known folder opens through launcher Enter'
    }
    $results.status=if($results.skipped.Count -gt 0) { 'passed_with_skips' } else { 'passed' }
} catch {
    $results.status='failed'; $results.failure=$_.Exception.Message
    throw
} finally {
    # No existing HWND is ever closed, and system processes are never killed.
    foreach($record in $ownedWindows) {
        $targetWindow=[IntPtr]::new($record.handle); $closed=$false
        try {
            if([SystemLauncherWin]::IsWindow($targetWindow) -and [SystemLauncherWin]::ProcessId($targetWindow) -eq $record.process_id -and -not $script:baselineHandles.Contains($record.handle)) {
                [void][SystemLauncherWin]::PostMessage($targetWindow,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
                Wait-For { -not [SystemLauncherWin]::IsWindow($targetWindow) -or -not [SystemLauncherWin]::IsWindowVisible($targetWindow) } "close only the new $($record.target) window" 2000
            }
            $closed=-not [SystemLauncherWin]::IsWindowVisible($targetWindow)
            $results.cleanup += [ordered]@{target=$record.target;handle=$record.handle;closed=$closed}
        } catch { $results.cleanup += [ordered]@{target=$record.target;handle=$record.handle;closed=$false;error=$_.Exception.Message} }
    }
    if($gui -and -not $gui.HasExited) { $gui.Kill(); [void]$gui.WaitForExit(2000) }
    if($shell -and [Runtime.InteropServices.Marshal]::IsComObject($shell)) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shell) }
    $results.foreground_restored=[SystemLauncherWin]::Focus($originalForeground)
    $results | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $runRoot 'results.json') -Encoding utf8NoBOM
    Write-Output "Evidence: $runRoot"
}
