param([string]$ExpectedVersion, [switch]$ApplyUpdate)
$ErrorActionPreference='Stop'
if(-not $ExpectedVersion -or $ExpectedVersion -notmatch '^\d+\.\d+\.\d+$') { throw 'ExpectedVersion must be x.y.z' }
if(-not $ApplyUpdate) { throw 'Pass -ApplyUpdate to explicitly exercise the running daily GUI updater' }
$projectRoot=Split-Path -Parent $PSScriptRoot
$dailyExe=Join-Path $projectRoot 'run/ptools.exe'
$reportRoot=Join-Path $projectRoot "artifacts/acceptance-v$ExpectedVersion"
New-Item -ItemType Directory -Path $reportRoot -Force | Out-Null
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class DailyUpdateWin {
  public delegate bool EnumProc(IntPtr h,IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb,IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder value,int max);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern bool SetWindowText(IntPtr h,string text);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder text,int max);
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h,int id);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW")] static extern IntPtr SendRaw(IntPtr h,uint msg,IntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW",CharSet=CharSet.Unicode)] static extern IntPtr SendText(IntPtr h,uint msg,IntPtr w,StringBuilder l,uint flags,uint timeout,out UIntPtr result);
  public static IntPtr[] Windows(uint process,string prefix) {
    var results=new List<IntPtr>();
    EnumWindows((h,l)=>{GetWindowThreadProcessId(h,out uint pid);var name=new StringBuilder(256);GetClassName(h,name,256);
      if(pid==process && name.ToString().StartsWith(prefix,StringComparison.Ordinal)) results.Add(h);return true;},IntPtr.Zero);
    return results.ToArray();
  }
  public static long Send(IntPtr h,uint msg,long w=0) {
    if(h==IntPtr.Zero || SendRaw(h,msg,new IntPtr(w),IntPtr.Zero,2,2000,out UIntPtr result)==IntPtr.Zero)
      throw new Exception("Daily launcher message failed: "+msg);
    return unchecked((long)result.ToUInt64());
  }
  public static string[] Rows(IntPtr list) {
    int count=(int)Send(list,0x18B);var rows=new List<string>();
    if(count<0 || count>10000) throw new Exception("Invalid launcher list count");
    for(int i=0;i<count;i++) {
      var value=new StringBuilder(4096);
      if(SendText(list,0x189,new IntPtr(i),value,2,2000,out UIntPtr result)==IntPtr.Zero) throw new Exception("Cannot read launcher row");
      rows.Add(value.ToString());
    }
    return rows.ToArray();
  }
  public static string Text(IntPtr h) {var value=new StringBuilder(4096);GetWindowText(h,value,4096);return value.ToString();}
}
'@
function Wait-For([scriptblock]$Condition,[string]$Name,[int]$TimeoutMs=30000) {
    $timer=[Diagnostics.Stopwatch]::StartNew()
    while($timer.ElapsedMilliseconds -lt $TimeoutMs) { if(& $Condition) { return }; Start-Sleep -Milliseconds 100 }
    throw "Timeout: $Name"
}
$running=@(Get-CimInstance Win32_Process -Filter "Name='ptools.exe'" | Where-Object ExecutablePath -eq $dailyExe)
if($running.Count -ne 1) { throw 'Expected exactly one daily instance at run/ptools.exe' }
$originalPid=[uint32]$running[0].ProcessId
$hosts=@([DailyUpdateWin]::Windows($originalPid,'ptools.window.'))
if($hosts.Count -ne 1) {throw 'Cannot uniquely identify the running launcher window'}
$hostWindow=$hosts[0]
$edit=[DailyUpdateWin]::GetDlgItem($hostWindow,201)
$list=[DailyUpdateWin]::GetDlgItem($hostWindow,202)
if($edit -eq [IntPtr]::Zero -or $list -eq [IntPtr]::Zero) {throw 'Daily launcher controls unavailable'}
[void][DailyUpdateWin]::Send($hostWindow,0x8001)
if(-not [DailyUpdateWin]::SetWindowText($edit,'更新')) {throw 'Cannot search the running launcher'}
Wait-For { $script:rows=@([DailyUpdateWin]::Rows($list)); @($rows | Where-Object {$_ -eq '检查更新' -or $_ -eq "更新到 $ExpectedVersion"}).Count -gt 0 } 'update search result'
if($rows -notcontains "更新到 $ExpectedVersion") {
    $index=[Array]::LastIndexOf($rows,'检查更新')
    [void][DailyUpdateWin]::Send($list,0x186,$index)
    [void][DailyUpdateWin]::PostMessage($hostWindow,0x8004,[IntPtr](13),[IntPtr]::Zero)
    Wait-For { $script:rows=@([DailyUpdateWin]::Rows($list)); $rows -contains "更新到 $ExpectedVersion" } 'published version discovered by running GUI' 60000
}
$index=[Array]::LastIndexOf($rows,"更新到 $ExpectedVersion")
[void][DailyUpdateWin]::Send($list,0x186,$index)
[void][DailyUpdateWin]::PostMessage($hostWindow,0x8004,[IntPtr](13),[IntPtr]::Zero)
Wait-For { $script:dialogs=@([DailyUpdateWin]::Windows($originalPid,'#32770')); $dialogs.Count -eq 1 } 'update confirmation'
$dialog=$dialogs[0]
$text=[DailyUpdateWin]::Text([DailyUpdateWin]::GetDlgItem($dialog,65535))
if($text -notlike "*更新到 $ExpectedVersion*" -or $text -notlike '*下载并校验完整软件包*') {throw 'Unexpected dialog; no confirmation was submitted'}
[void][DailyUpdateWin]::PostMessage($dialog,0x111,[IntPtr](6),[IntPtr]::Zero)
Wait-For {
    $script:after=@(Get-CimInstance Win32_Process -Filter "Name='ptools.exe'" | Where-Object { $_.ExecutablePath -eq $dailyExe -and $_.ProcessId -ne $originalPid })
    [Diagnostics.FileVersionInfo]::GetVersionInfo($dailyExe).ProductVersion -eq $ExpectedVersion -and $after.Count -eq 1
} 'complete download, verification, replacement and restart' 90000
$result=@{status='passed';expectedVersion=$ExpectedVersion;originalPid=$originalPid;newPid=$after[0].ProcessId;path=$dailyExe;method='running GUI searches update, checks published release, confirms download and checksum, then restarts itself';log=(Get-Content -LiteralPath (Join-Path $projectRoot 'run/data/last-update.txt') -Raw).Trim()}
$result | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $reportRoot 'gui-update.json')
$result | ConvertTo-Json
