param([string]$Executable = '', [switch]$SkipTimeout, [switch]$UiOnly, [string]$ExistingDataRoot='', [switch]$RealKeyboard)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $Executable) { $Executable = Join-Path $projectRoot 'dist/ptools/ptools.exe' }
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$runRoot = Join-Path $projectRoot ('artifacts/verify-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
$dataRoot = Join-Path $runRoot 'data'
if($ExistingDataRoot) { $dataRoot=(Resolve-Path -LiteralPath $ExistingDataRoot).Path }
New-Item -ItemType Directory -Path $runRoot -Force | Out-Null
$results = [ordered]@{ timestamp = (Get-Date).ToString('o'); executable = $Executable; checks = @() }
$results.keyboard_mode = if($RealKeyboard) { 'SendInput' } else { 'WM_HOTKEY dispatch; registration checked independently' }
function Check([bool]$Ok, [string]$Name) {
    $script:results.checks += [ordered]@{ name = $Name; passed = $Ok }
    if (-not $Ok) { throw "FAILED: $Name" }
    Write-Output "PASS: $Name"
}
function Run-Cli([string[]]$Arguments, [bool]$ExpectSuccess = $true) {
    $info = [Diagnostics.ProcessStartInfo]::new($Executable)
    $info.UseShellExecute = $false; $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
    $info.ArgumentList.Add('--data-dir'); $info.ArgumentList.Add($dataRoot)
    foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($info)
    $output = $process.StandardOutput.ReadToEnd(); $errorText = $process.StandardError.ReadToEnd()
    if (-not $process.WaitForExit(45000)) { $process.Kill(); throw 'CLI timeout' }
    if ($ExpectSuccess -and $process.ExitCode -ne 0) { throw "CLI failed ($($process.ExitCode)): $output $errorText" }
    return [pscustomobject]@{ ExitCode=$process.ExitCode; Output=$output; Error=$errorText }
}

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class PToolsWin {
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint from,uint to,bool attach);
  public delegate bool EnumProc(IntPtr hwnd, IntPtr lparam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr lparam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr hwnd, int id);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr hwnd, uint msg, IntPtr w, string l);
  [DllImport("user32.dll", EntryPoint="SendMessageW")] public static extern IntPtr Send(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool RegisterHotKey(IntPtr hwnd, int id, uint modifiers, uint key);
  [DllImport("user32.dll")] public static extern bool UnregisterHotKey(IntPtr hwnd, int id);
  [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint thread, ref GUIINFO info);
  [DllImport("user32.dll")] public static extern uint SendInput(uint count, INPUT[] inputs, int size);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct GUIINFO { public int size, flags; public IntPtr active, focus, capture, menuOwner, moveSize, caret; public RECT rect; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public UNION value; }
  [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public KEY key; [FieldOffset(0)] public MOUSE mouse; }
  [StructLayout(LayoutKind.Sequential)] public struct KEY { public ushort vk, scan; public uint flags, time; public UIntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSE { public int x,y; public uint data,flags,time; public UIntPtr extra; }
  public static void Hotkey() {
    ushort[] keys = { 0x11, 0x12, 0x20, 0x20, 0x12, 0x11 };
    var input = new INPUT[6];
    for (int i=0;i<6;i++) { input[i].type=1; input[i].value.key.vk=keys[i]; input[i].value.key.flags=i<3?0u:2u; }
    if(SendInput(6,input,Marshal.SizeOf<INPUT>())!=6) throw new Exception("SendInput failed");
  }
  public static bool Focus(IntPtr target) {
    uint pid;
    uint current=GetCurrentThreadId(), foreground=GetWindowThreadProcessId(GetForegroundWindow(),out pid), destination=GetWindowThreadProcessId(target,out pid);
    bool a=current!=foreground && AttachThreadInput(current,foreground,true);
    bool b=current!=destination && destination!=foreground && AttachThreadInput(current,destination,true);
    try { return SetForegroundWindow(target); }
    finally { if(b) AttachThreadInput(current,destination,false); if(a) AttachThreadInput(current,foreground,false); }
  }
  public static IntPtr Find(uint processId) {
    IntPtr result=IntPtr.Zero;
    EnumWindows((hwnd,l) => { uint found; GetWindowThreadProcessId(hwnd,out found); if(found==processId && GetDlgItem(hwnd,201)!=IntPtr.Zero) { result=hwnd; return false; } return true; },IntPtr.Zero);
    return result;
  }
}
'@

function Wait-For([scriptblock]$Condition, [int]$TimeoutMs = 10000) {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    while (-not (& $Condition)) { if ($watch.ElapsedMilliseconds -gt $TimeoutMs) { throw "Condition timeout ($script:phase): $Condition" }; Start-Sleep -Milliseconds 20 }
}
function Capture([IntPtr]$Window, [string]$Name) {
    Start-Sleep -Milliseconds 350
    $oldDpi=[PToolsWin]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $rectangle = [PToolsWin+RECT]::new(); [void][PToolsWin]::GetWindowRect($Window, [ref]$rectangle)
    $bitmap = [Drawing.Bitmap]::new($rectangle.Right-$rectangle.Left, $rectangle.Bottom-$rectangle.Top)
    $graphics = [Drawing.Graphics]::FromImage($bitmap); $dc=$graphics.GetHdc()
    try { [void][PToolsWin]::PrintWindow($Window,$dc,2) } finally { $graphics.ReleaseHdc($dc) }
    $bitmap.Save((Join-Path $runRoot $Name),[Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bitmap.Dispose()
    [void][PToolsWin]::SetThreadDpiAwarenessContext($oldDpi)
}

function Invoke-Hotkey {
    if($RealKeyboard) { [PToolsWin]::Hotkey() }
    else { [void][PToolsWin]::PostMessage($window,0x0312,[IntPtr]101,[IntPtr]::Zero) }
}

$gui = $null
$originalForeground=[PToolsWin]::GetForegroundWindow()
try {
    if(-not $UiOnly) {
    $refresh = Run-Cli @('--refresh-index')
    $cache = Get-Content -LiteralPath (Join-Path $dataRoot 'index.json') -Raw | ConvertFrom-Json
    Check ($cache.plugins.applications.Count -gt 0) 'Real Windows application discovery'
    $results.application_count = $cache.plugins.applications.Count
    $results.application_types = @($cache.plugins.applications | Group-Object { $_.target.kind } | ForEach-Object { [ordered]@{ kind=$_.Name; count=$_.Count } })
    $probe = Join-Path $runRoot 'launch-probe.exe'
    & rustc (Join-Path $PSScriptRoot 'launch-probe.rs') -O -o $probe
    if ($LASTEXITCODE -ne 0) { throw 'Probe build failed' }
    $shortcutPath=Join-Path $runRoot 'launch-probe.lnk'
    $shortcutShell=New-Object -ComObject WScript.Shell
    $shortcut=$shortcutShell.CreateShortcut($shortcutPath); $shortcut.TargetPath=$probe; $shortcut.Save()
    $configSource = Join-Path $runRoot 'config-plugin'; New-Item -ItemType Directory -Path $configSource | Out-Null
    [ordered]@{schema_version=1;id='launch-probe';name='验证启动';version='1';runtime='config';entries=@(@{id='probe';title='测试启动探针';target=@{kind='path';path=$shortcutPath.Replace('\','/')}})} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $configSource 'plugin.json') -Encoding utf8NoBOM
    $package = Join-Path $runRoot 'probe.zip'; Compress-Archive -Path (Join-Path $configSource 'plugin.json') -DestinationPath $package
    $null = Run-Cli @('--install',$package)
    foreach ($query in @('测试启动探针','ceshiqidongtanzhen','csqdtz')) {
        $hits=(Run-Cli @('--search',$query)).Output | ConvertFrom-Json
        Check (@($hits).Count -eq 1 -and $hits[0].id -eq 'probe') "Search: $query"
    }
    $nativeSource = Join-Path $runRoot 'native-plugin'; New-Item -ItemType Directory -Path $nativeSource | Out-Null
    Copy-Item -LiteralPath $probe -Destination (Join-Path $nativeSource 'probe.exe')
    'valid' | Set-Content -LiteralPath (Join-Path $nativeSource 'mode.txt') -Encoding utf8NoBOM
    @{schema_version=1;id='native-probe';name='原生探针';version='1';runtime='executable';executable='probe.exe'} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $nativeSource 'plugin.json') -Encoding utf8NoBOM
    $null = Run-Cli @('--install',$nativeSource)
    Check (((Run-Cli @('--search','原生协议探针')).Output | ConvertFrom-Json).Count -eq 1) 'Native EXE protocol'
    $modePath=Join-Path $dataRoot 'plugins/native-probe/mode.txt'
    foreach ($mode in @('invalid','flood')) {
        $mode | Set-Content -LiteralPath $modePath -Encoding utf8NoBOM
        $bad=Run-Cli @('--refresh-index') $false
        Check ($bad.ExitCode -ne 0) "Reject native response: $mode"
        Check (((Run-Cli @('--search','原生协议探针')).Output | ConvertFrom-Json).Count -eq 1) "Retain last good index: $mode"
    }
    if (-not $SkipTimeout) {
        'hang' | Set-Content -LiteralPath $modePath -Encoding utf8NoBOM
        $watch=[Diagnostics.Stopwatch]::StartNew(); $bad=Run-Cli @('--refresh-index') $false
        Check ($bad.ExitCode -ne 0 -and $watch.Elapsed.TotalSeconds -lt 40) 'Terminate timed-out plugin'
    }
    'hang' | Set-Content -LiteralPath $modePath -Encoding utf8NoBOM
    $abruptInfo=[Diagnostics.ProcessStartInfo]::new($Executable); $abruptInfo.UseShellExecute=$false; $abruptInfo.CreateNoWindow=$true; $abruptInfo.WindowStyle=[Diagnostics.ProcessWindowStyle]::Hidden
    foreach($arg in @('--data-dir',$dataRoot,'--refresh-index')) { $abruptInfo.ArgumentList.Add($arg) }
    $abrupt=[Diagnostics.Process]::Start($abruptInfo)
    $pluginProcess=$null
    try {
        Wait-For { $script:childRecord=Get-CimInstance Win32_Process -Filter "ParentProcessId=$($abrupt.Id) AND Name='probe.exe'"; $null -ne $script:childRecord }
        $pluginProcess=[Diagnostics.Process]::GetProcessById($script:childRecord.ProcessId)
        $abrupt.Kill(); $abrupt.WaitForExit()
        Check ($pluginProcess.WaitForExit(5000)) 'Job Object reclaims plugin when host terminates'
    } finally {
        if(-not $abrupt.HasExited) { $abrupt.Kill() }
        if($pluginProcess -and -not $pluginProcess.HasExited) { $pluginProcess.Kill() }
    }
    $null=Run-Cli @('--uninstall','native-probe')
    }
    $settingsPath=Join-Path $dataRoot 'settings.json'
    $settings=Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json
    $settings.hotkey='Ctrl+Alt+Space'; $settings | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $settingsPath -Encoding utf8NoBOM
    if($RealKeyboard) { $results.desktop_focus_requested=[PToolsWin]::Focus([PToolsWin]::GetShellWindow()) }
    $env:PTOOLS_TEST_MARKER=Join-Path $runRoot 'launched.txt'
    $info=[Diagnostics.ProcessStartInfo]::new($Executable); $info.UseShellExecute=$false; $info.CreateNoWindow=$true; $info.WindowStyle=[Diagnostics.ProcessWindowStyle]::Hidden
    foreach($arg in @('--data-dir',$dataRoot,'--hidden')) { $info.ArgumentList.Add($arg) }
    $gui=[Diagnostics.Process]::Start($info)
    Wait-For { [PToolsWin]::Find($gui.Id) -ne [IntPtr]::Zero }
    $window=[PToolsWin]::Find($gui.Id); $edit=[PToolsWin]::GetDlgItem($window,201); $list=[PToolsWin]::GetDlgItem($window,202)
    Wait-For { @(Get-Process -Name ptools-applications -ErrorAction SilentlyContinue).Count -eq 0 }
    Check (-not [PToolsWin]::IsWindowVisible($window)) 'Hidden startup'
    Check ([PToolsWin]::GetForegroundWindow() -ne $window) 'Hidden window does not take foreground focus'
    $available=[PToolsWin]::RegisterHotKey([IntPtr]::Zero,999,0x4003,0x20)
    if($available) { [void][PToolsWin]::UnregisterHotKey([IntPtr]::Zero,999) }
    Check (-not $available) 'Global hotkey registered by host'
    $again=Run-Cli @('--hidden')
    Check ($again.ExitCode -eq 0) 'Second instance forwards to existing window'
    $script:phase='second-instance visible'; Wait-For { [PToolsWin]::IsWindowVisible($window) }
    Start-Sleep -Milliseconds 150
    $script:phase='hide second-instance'; Invoke-Hotkey; Wait-For { -not [PToolsWin]::IsWindowVisible($window) }
    $latencies=@()
    for($i=0;$i -lt 12;$i++) {
        Start-Sleep -Milliseconds 100
        $script:phase="hotkey show $i"
        $watch=[Diagnostics.Stopwatch]::StartNew(); Invoke-Hotkey
        Wait-For { [PToolsWin]::IsWindowVisible($window) }
        $latencies+=$watch.Elapsed.TotalMilliseconds
        $results.hotkey_samples_ms=$latencies
        Start-Sleep -Milliseconds 100
        $script:phase="hotkey hide $i"
        Invoke-Hotkey; Wait-For { -not [PToolsWin]::IsWindowVisible($window) }
    }
    $results.hotkey_samples_ms=$latencies; $results.hotkey_p95_ms=($latencies | Sort-Object)[[Math]::Ceiling($latencies.Count*0.95)-1]
    Invoke-Hotkey; Wait-For { [PToolsWin]::IsWindowVisible($window) }
    $guiInfo=[PToolsWin+GUIINFO]::new(); $guiInfo.size=[Runtime.InteropServices.Marshal]::SizeOf($guiInfo)
    [uint32]$foundPid=0; $thread=[PToolsWin]::GetWindowThreadProcessId($window,[ref]$foundPid)
    [void][PToolsWin]::GetGUIThreadInfo($thread,[ref]$guiInfo)
    Check ($guiInfo.focus -eq $edit) 'Search input focused after hotkey'
    if($RealKeyboard) { Check ([PToolsWin]::GetForegroundWindow() -eq $window) 'Hotkey brings search to foreground' }
    $queryTimes=@()
    foreach($query in @('code','微信','wx','csqdtz','chrome','记事本','jsb','测试启动探针','不存在的应用','计算器')) {
        $watch=[Diagnostics.Stopwatch]::StartNew()
        [void][PToolsWin]::SendMessage($edit,0x000C,[IntPtr]::Zero,$query)
        $queryTimes+=$watch.Elapsed.TotalMilliseconds
    }
    $results.query_processing_samples_ms=$queryTimes
    [void][PToolsWin]::SendMessage($edit,0x000C,[IntPtr]::Zero,'csqdtz')
    Check ([PToolsWin]::Send($list,0x018B,[IntPtr]::Zero,[IntPtr]::Zero).ToInt64() -eq 1) 'Native list displays filtered result'
    Capture $window 'search-probe.png'
    [void][PToolsWin]::PostMessage($edit,0x0100,[IntPtr]0x0D,[IntPtr]::Zero)
    Wait-For { Test-Path -LiteralPath $env:PTOOLS_TEST_MARKER }
    Check (-not [PToolsWin]::IsWindowVisible($window)) 'Enter launches real process and hides search'
    Check ([PToolsWin]::GetForegroundWindow() -ne $window) 'Launching does not refocus hidden search'
    Invoke-Hotkey; Wait-For { [PToolsWin]::IsWindowVisible($window) }
    [void][PToolsWin]::SendMessage($edit,0x000C,[IntPtr]::Zero,'code'); Capture $window 'search-applications.png'
    [void][PToolsWin]::SendMessage($edit,0x000C,[IntPtr]::Zero,'插件')
    [void][PToolsWin]::PostMessage($edit,0x0100,[IntPtr]0x0D,[IntPtr]::Zero)
    Start-Sleep -Milliseconds 150; Capture $window 'plugins.png'
    Check ([PToolsWin]::Send($list,0x018B,[IntPtr]::Zero,[IntPtr]::Zero).ToInt64() -ge 3) 'Plugin management UI'
    [void][PToolsWin]::Send($list,0x0186,[IntPtr]1,[IntPtr]::Zero)
    [void][PToolsWin]::PostMessage($edit,0x0100,[IntPtr]0x0D,[IntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    [void][PToolsWin]::PostMessage($edit,0x0100,[IntPtr]0x0D,[IntPtr]::Zero)
    Wait-For { $current=Get-Content -LiteralPath (Join-Path $dataRoot 'index.json') -Raw | ConvertFrom-Json; -not $current.plugins.applications }
    Check (((Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json).disabled_plugins) -contains 'applications') 'Disable application plugin from UI'
    Start-Sleep -Milliseconds 100
    [void][PToolsWin]::PostMessage($edit,0x0100,[IntPtr]0x0D,[IntPtr]::Zero)
    Wait-For { $current=Get-Content -LiteralPath (Join-Path $dataRoot 'index.json') -Raw | ConvertFrom-Json; $current.plugins.applications.Count -gt 0 }
    Check (-not (((Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json).disabled_plugins) -contains 'applications')) 'Enable application plugin from UI'
    [void][PToolsWin]::PostMessage($edit,0x0100,[IntPtr]0x1B,[IntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    [void][PToolsWin]::PostMessage($edit,0x0100,[IntPtr]0x1B,[IntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    [void][PToolsWin]::SendMessage($edit,0x000C,[IntPtr]::Zero,'设置')
    [void][PToolsWin]::PostMessage($edit,0x0100,[IntPtr]0x0D,[IntPtr]::Zero)
    Start-Sleep -Milliseconds 150; Capture $window 'settings.png'
    Check ([PToolsWin]::Send($list,0x018B,[IntPtr]::Zero,[IntPtr]::Zero).ToInt64() -eq 8) 'Settings UI'
    [void][PToolsWin]::PostMessage($edit,0x0100,[IntPtr]0x1B,[IntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    [void][PToolsWin]::PostMessage($edit,0x0100,[IntPtr]0x1B,[IntPtr]::Zero)
    Wait-For { -not [PToolsWin]::IsWindowVisible($window) }
    $gui.Refresh(); $cpuStart=$gui.TotalProcessorTime.TotalMilliseconds; $sample=[Diagnostics.Stopwatch]::StartNew()
    Start-Sleep -Seconds 5
    $gui.Refresh(); $cpuMs=$gui.TotalProcessorTime.TotalMilliseconds-$cpuStart
    $results.idle=[ordered]@{working_set_mb=[Math]::Round($gui.WorkingSet64/1MB,2);private_commit_mb=[Math]::Round($gui.PrivateMemorySize64/1MB,2);cpu_ms=$cpuMs;sample_ms=$sample.Elapsed.TotalMilliseconds}
    $counter=Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($gui.Id)" -ErrorAction SilentlyContinue
    if($counter) { $results.idle.private_working_set_mb=[Math]::Round($counter.WorkingSetPrivate/1MB,2) }
    Check (@(Get-Process -Name ptools-applications -ErrorAction SilentlyContinue).Count -eq 0) 'Application discovery plugin exits after refresh'
    Invoke-Hotkey; Wait-For { [PToolsWin]::IsWindowVisible($window) }
    [void][PToolsWin]::SendMessage($edit,0x000C,[IntPtr]::Zero,'退出')
    [void][PToolsWin]::PostMessage($edit,0x0100,[IntPtr]0x0D,[IntPtr]::Zero)
    Check ($gui.WaitForExit(5000)) 'Host clean exit'
    $gui=$null
    $released=[PToolsWin]::RegisterHotKey([IntPtr]::Zero,999,0x4003,0x20)
    if($released) { [void][PToolsWin]::UnregisterHotKey([IntPtr]::Zero,999) }
    Check ($released) 'Global hotkey released after exit'
    $null=Run-Cli @('--uninstall','launch-probe')
    Check (((Run-Cli @('--search','csqdtz')).Output | ConvertFrom-Json).Count -eq 0) 'Uninstall removes search entries'
    $savedSettings=Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json
    Check (-not ($savedSettings.usage.PSObject.Properties.Name -contains 'launch-probe:probe')) 'Uninstall removes plugin usage records'
} finally {
    if($gui -and -not $gui.HasExited) { $gui.Kill() }
    [void][PToolsWin]::Focus($originalForeground)
    $results | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $runRoot 'results.json') -Encoding utf8NoBOM
    Write-Output "Evidence: $runRoot"
}
