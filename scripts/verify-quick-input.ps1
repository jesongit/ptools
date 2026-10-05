param([string]$Executable='', [switch]$VerifyClipboard)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $Executable) { $Executable=Join-Path $projectRoot 'dist/ptools/ptools.exe' }
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$runRoot=Join-Path $projectRoot ('artifacts/quick-input-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8))
$dataRoot=Join-Path $runRoot 'data'
New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
@{hotkey='Ctrl+Alt+Space';disabled_plugins=@('capture','uninstaller');usage=@{};plugin_hotkeys=@{}} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $dataRoot 'settings.json') -Encoding utf8NoBOM
$results=[ordered]@{timestamp=(Get-Date).ToString('o');executable=$Executable;data_dir=$dataRoot;status='running';checks=@();skipped=@();clipboard_requested=$VerifyClipboard.IsPresent;clipboard_restored=$null;controls=@{input=201;list=202;menu=203;copy=204;result=205;metadata=206}}
$gui=$null; $originalForeground=[IntPtr]::Zero; $clipboardReady=$false; $clipboardTouched=$false
$clipboardSnapshot=$null; $clipboardWasEmpty=$false
$clipboardCopies=[Collections.Generic.List[IDisposable]]::new()
function Check([bool]$Ok,[string]$Name,[object]$Evidence=$null) {
    $script:results.checks += [ordered]@{name=$Name;passed=$Ok;evidence=$Evidence}
    if(-not $Ok) { throw "FAILED: $Name" }; Write-Output "PASS: $Name"
}
function Skip([string]$Name,[string]$Reason) {
    $script:results.skipped += [ordered]@{name=$Name;reason=$Reason}; Write-Output "SKIP: $Name ($Reason)"
}
function Wait-For([scriptblock]$Condition,[string]$Name,[int]$TimeoutMs=10000) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while($watch.ElapsedMilliseconds -lt $TimeoutMs) {
        if(& $Condition) { return }; Start-Sleep -Milliseconds 40
    }
    throw "Timeout after $TimeoutMs ms: $Name"
}
function Remains-True([scriptblock]$Condition,[string]$Name,[int]$DurationMs=600) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while($watch.ElapsedMilliseconds -lt $DurationMs) {
        if(-not (& $Condition)) { Check $false $Name }; Start-Sleep -Milliseconds 40
    }
    Check $true $Name
}
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class QuickInputWin {
  public delegate bool EnumProc(IntPtr h,IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h,int id);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll",EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr h,int index);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW")] private static extern IntPtr SendRaw(IntPtr h,uint msg,IntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW",CharSet=CharSet.Unicode)] private static extern IntPtr SendText(IntPtr h,uint msg,IntPtr w,string l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW",CharSet=CharSet.Unicode)] private static extern IntPtr SendBuffer(IntPtr h,uint msg,IntPtr w,StringBuilder l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
  [DllImport("user32.dll")] public static extern bool RedrawWindow(IntPtr h,IntPtr updateRect,IntPtr updateRegion,uint flags);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] private static extern IntPtr SetFocus(IntPtr h);
  [DllImport("user32.dll")] private static extern IntPtr GetFocus();
  [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] private static extern bool AttachThreadInput(uint from,uint to,bool attach);
  [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
  [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
  [DllImport("user32.dll")] private static extern uint SendInput(uint count,INPUT[] inputs,int size);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint Type; public UNION Data; }
  [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public KEY Key; [FieldOffset(0)] public MOUSE Mouse; }
  [StructLayout(LayoutKind.Sequential)] public struct KEY { public ushort Vk,Scan; public uint Flags,Time; public UIntPtr Extra; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSE { public int X,Y; public uint Data,Flags,Time; public UIntPtr Extra; }
  public static IntPtr FindHost(uint pid) {
    IntPtr result=IntPtr.Zero;
    EnumWindows((h,l)=>{GetWindowThreadProcessId(h,out uint found);if(found==pid && GetDlgItem(h,201)!=IntPtr.Zero){result=h;return false;}return true;},IntPtr.Zero);
    return result;
  }
  public static long Send(IntPtr h,uint msg,long w=0,long l=0) {
    if(SendRaw(h,msg,new IntPtr(w),new IntPtr(l),2,1000,out UIntPtr result)==IntPtr.Zero) throw new Exception("Window message failed or timed out: "+msg);
    return unchecked((long)result.ToUInt64());
  }
  public static void SetText(IntPtr h,string text) {
    if(SendText(h,0x000C,IntPtr.Zero,text,2,1000,out UIntPtr result)==IntPtr.Zero) throw new Exception("WM_SETTEXT failed or timed out");
  }
  public static void ReplaceSelection(IntPtr h,string text) {
    if(SendText(h,0x00C2,new IntPtr(1),text,2,1000,out UIntPtr result)==IntPtr.Zero) throw new Exception("EM_REPLACESEL failed or timed out");
  }
  public static string Text(IntPtr h) {
    long length=Send(h,0x000E);
    if(length<0 || length>4194304) throw new Exception("Invalid control text length");
    var text=new StringBuilder((int)length+1);
    if(SendBuffer(h,0x000D,new IntPtr(text.Capacity),text,2,1000,out UIntPtr result)==IntPtr.Zero) throw new Exception("WM_GETTEXT failed or timed out");
    return text.ToString();
  }
  public static string ListTitle(IntPtr h,int index) {
    long length=Send(h,0x018A,index);
    if(length<0 || length>32760) throw new Exception("Invalid list title length");
    var text=new StringBuilder((int)length+1);
    if(SendBuffer(h,0x0189,new IntPtr(index),text,2,1000,out UIntPtr result)==IntPtr.Zero) throw new Exception("LB_GETTEXT failed or timed out");
    return text.ToString();
  }
  public static bool Focus(IntPtr window,IntPtr edit) {
    if(!IsWindow(window)) return false;
    uint current=GetCurrentThreadId(), foreground=GetWindowThreadProcessId(GetForegroundWindow(),out uint first), destination=GetWindowThreadProcessId(window,out uint second);
    bool a=current!=foreground && AttachThreadInput(current,foreground,true), b=current!=destination && destination!=foreground && AttachThreadInput(current,destination,true);
    try { SetForegroundWindow(window); if(edit!=IntPtr.Zero) SetFocus(edit); return GetForegroundWindow()==window && (edit==IntPtr.Zero || GetFocus()==edit); }
    finally { if(b) AttachThreadInput(current,destination,false); if(a) AttachThreadInput(current,foreground,false); }
  }
  public static void CtrlShiftC() {
    ushort[] keys={0x11,0x10,0x43,0x43,0x10,0x11}; var input=new INPUT[6];
    for(int i=0;i<6;i++) input[i]=new INPUT {Type=1,Data=new UNION {Key=new KEY {Vk=keys[i],Flags=i<3?0u:2u}}};
    if(SendInput(6,input,Marshal.SizeOf<INPUT>())!=6) {
      var release=new INPUT[3];
      for(int i=0;i<3;i++) release[i]=new INPUT {Type=1,Data=new UNION {Key=new KEY {Vk=keys[i+3],Flags=2}}};
      SendInput(3,release,Marshal.SizeOf<INPUT>()); throw new Exception("Ctrl+Shift+C SendInput failed");
    }
  }
}
'@
function Output-Text { [QuickInputWin]::Text($script:resultEdit) }
function Metadata-Text { [QuickInputWin]::Text($script:metadata) }
function Normalize-Output([string]$Text) { $Text.Replace("`r`n","`n").TrimEnd([char[]]@("`r","`n")) }
function Set-Query([string]$Query) {
    if(-not [QuickInputWin]::IsWindowVisible($script:hostWindow)) {
        [void][QuickInputWin]::PostMessage($script:hostWindow,0x0312,[IntPtr]101,[IntPtr]::Zero)
        Wait-For { [QuickInputWin]::IsWindowVisible($script:hostWindow) } 'launcher visible'
    }
    $panel=[QuickInputWin]::GetDlgItem($script:hostWindow,205)
    if([QuickInputWin]::IsWindowVisible($panel)) {
        [void][QuickInputWin]::PostMessage($script:edit,0x0100,[IntPtr]0x1B,[IntPtr]::Zero)
        Wait-For { -not [QuickInputWin]::IsWindowVisible($panel) -and [QuickInputWin]::IsWindowVisible($script:hostWindow) } 'leave tool mode before setting a new search query'
    }
    [QuickInputWin]::SetText($script:edit,$Query)
}
function Set-InputBody([string]$Body) { [QuickInputWin]::SetText($script:edit,$Body) }
function Delete-InputBody {
    [void][QuickInputWin]::Send($script:edit,0x00B1,0,-1)
    [QuickInputWin]::ReplaceSelection($script:edit,'')
}
function Press-InputKey([int]$Key,[long]$KeyFlags=0,[switch]$Character) {
    [void][QuickInputWin]::PostMessage($script:edit,0x0100,[IntPtr]$Key,[IntPtr]$KeyFlags)
    if($Character) { [void][QuickInputWin]::PostMessage($script:edit,0x0102,[IntPtr]$Key,[IntPtr]$KeyFlags) }
}
function Wait-RestoredSearch([string]$Query) {
    Wait-For { [QuickInputWin]::IsWindowVisible($script:hostWindow) -and -not [QuickInputWin]::IsWindowVisible($script:resultEdit) -and [QuickInputWin]::IsWindowVisible($script:list) -and [QuickInputWin]::Text($script:edit) -eq $Query } "restore previous search query: $Query"
}
function Enter-Query { [void][QuickInputWin]::PostMessage($script:edit,0x0100,[IntPtr]0x0D,[IntPtr]::Zero) }
function Wait-Command([string]$Sentinel,[int]$ExitCode=0) {
    Wait-For { (Output-Text).Contains($Sentinel) -and (Metadata-Text) -match ("退出码\s*[:：]?\s*" + $ExitCode + '(?!\d)') -and [QuickInputWin]::IsWindowEnabled($script:copyButton) } "command completes with exit code $ExitCode"
    Check ([QuickInputWin]::IsWindowVisible($script:hostWindow)) 'Command result keeps launcher visible' ([ordered]@{metadata=(Metadata-Text);output=(Output-Text)})
}
function Snapshot([string]$Name) {
    $previousDpi=[QuickInputWin]::SetThreadDpiAwarenessContext([IntPtr](-4)); $bitmap=$null; $graphics=$null
    try {
        if(-not [QuickInputWin]::RedrawWindow($hostWindow,[IntPtr]::Zero,[IntPtr]::Zero,0x181)) { throw 'Cannot redraw launcher before capture' }
        $rect=[QuickInputWin+RECT]::new(); if(-not [QuickInputWin]::GetWindowRect($hostWindow,[ref]$rect)) { throw 'Cannot get launcher rectangle' }
        $bitmap=[Drawing.Bitmap]::new($rect.Right-$rect.Left,$rect.Bottom-$rect.Top); $graphics=[Drawing.Graphics]::FromImage($bitmap); $dc=$graphics.GetHdc()
        try { $printed=[QuickInputWin]::PrintWindow($hostWindow,$dc,2) } finally { $graphics.ReleaseHdc($dc) }
        if(-not $printed) { throw 'Cannot capture launcher' }
        $bitmap.Save((Join-Path $runRoot $Name),[Drawing.Imaging.ImageFormat]::Png)
    } finally {
        if($graphics) { $graphics.Dispose() }; if($bitmap) { $bitmap.Dispose() }; [void][QuickInputWin]::SetThreadDpiAwarenessContext($previousDpi)
    }
}
function Invoke-ClipboardWrite([scriptblock]$Action,[int]$TimeoutMs=3000) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while($true) {
        try { & $Action; return }
        catch {
            if($_.Exception.GetBaseException() -isnot [Runtime.InteropServices.ExternalException] -or $watch.ElapsedMilliseconds -ge $TimeoutMs) { throw }
            $script:results.clipboard_last_retry_error=$_.Exception.Message
        }
        Start-Sleep -Milliseconds 40
    }
}
function Clipboard-Matches([string]$Expected) {
    try { (Normalize-Output ([Windows.Forms.Clipboard]::GetText())) -eq (Normalize-Output $Expected) }
    catch {
        if($_.Exception.GetBaseException() -isnot [Runtime.InteropServices.ExternalException]) { throw }
        $script:results.clipboard_last_retry_error=$_.Exception.Message
        $false
    }
}
function Verify-Copy([string]$Name,[string]$Expected,[scriptblock]$Action) {
    if(-not $script:clipboardReady) { Skip $Name $script:clipboardSkipReason; return }
    $sentinel='ptools verification sentinel ' + [Guid]::NewGuid().ToString('N'); $script:clipboardTouched=$true
    Invoke-ClipboardWrite { [Windows.Forms.Clipboard]::SetText($sentinel) }
    & $Action
    Wait-For { Clipboard-Matches $Expected } $Name 3000
    Check $true $Name
}

try {
    $originalForeground=[QuickInputWin]::GetForegroundWindow()
    $clipboardSkipReason='Run with -VerifyClipboard to enable copy checks'
    if($VerifyClipboard) {
        if([Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') { $clipboardSkipReason='Clipboard checks require pwsh -STA; no clipboard was changed' }
        else {
            try {
                $sequence=[QuickInputWin]::GetClipboardSequenceNumber(); $source=[Windows.Forms.Clipboard]::GetDataObject()
                $clipboardSnapshot=[Windows.Forms.DataObject]::new(); $clipboardWasEmpty=$null -eq $source
                if($source) {
                    foreach($format in $source.GetFormats($false)) {
                        $value=$source.GetData($format,$false)
                        if($value -is [Drawing.Image]) { $value=$value.Clone(); $clipboardCopies.Add($value) }
                        elseif($value -is [IO.Stream]) {
                            if(-not $value.CanSeek) { throw "Cannot safely snapshot nonseekable stream: $format" }
                            $stream=[IO.MemoryStream]::new(); $position=$value.Position
                            try { $value.Position=0; $value.CopyTo($stream) } finally { $value.Position=$position }
                            $stream.Position=0; $value=$stream; $clipboardCopies.Add($value)
                        } elseif($value -is [Array]) { $value=$value.Clone() }
                        elseif($null -ne $value -and $value -isnot [string]) { throw "Unsupported clipboard format: $format" }
                        if($null -eq $value) { throw "Cannot materialize clipboard format: $format" }
                        $clipboardSnapshot.SetData($format,$false,$value)
                    }
                }
                if([QuickInputWin]::GetClipboardSequenceNumber() -ne $sequence) { throw 'Clipboard changed while its formats were being copied' }
                $clipboardReady=$true; $results.clipboard_snapshot='All advertised formats were materialized before any copy check'
            } catch { $clipboardSkipReason=$_.Exception.Message + '; copy checks skipped without changing clipboard'; $results.clipboard_snapshot=$clipboardSkipReason }
        }
    }
    $hostArguments=@('--hidden','--data-dir',('"' + $dataRoot + '"'))
    $gui=Start-Process -FilePath $Executable -ArgumentList $hostArguments -WindowStyle Hidden -PassThru; $results.host_process_id=$gui.Id
    Wait-For { $script:hostWindow=[QuickInputWin]::FindHost($gui.Id); $script:hostWindow -ne [IntPtr]::Zero } 'isolated launcher window'
    $script:edit=[QuickInputWin]::GetDlgItem($hostWindow,201); $script:list=[QuickInputWin]::GetDlgItem($hostWindow,202)
    Set-Query '=1+2*3'
    $script:copyButton=[QuickInputWin]::GetDlgItem($hostWindow,204); $script:resultEdit=[QuickInputWin]::GetDlgItem($hostWindow,205); $script:metadata=[QuickInputWin]::GetDlgItem($hostWindow,206)
    Check ($copyButton -ne [IntPtr]::Zero -and $resultEdit -ne [IntPtr]::Zero -and $metadata -ne [IntPtr]::Zero) 'Quick input controls exist'
    $resultStyle=[QuickInputWin]::GetWindowLongPtr($resultEdit,-16).ToInt64()
    Check (($resultStyle -band 0x0800) -ne 0 -and ($resultStyle -band 0x0004) -ne 0) 'Result EDIT is read-only and multiline'
    Wait-For { (Output-Text).Trim() -eq '7' -and [QuickInputWin]::IsWindowEnabled($copyButton) } 'live calculator result 7'
    Check $true 'Calculator respects multiplication precedence without Enter' ([ordered]@{query='=1+2*3';output=(Output-Text);metadata=(Metadata-Text)})
    Check ([QuickInputWin]::Text($edit) -eq '1+2*3') 'Calculator trigger is removed from the input body'
    Snapshot 'calculator.png'
    Verify-Copy 'Calculator Enter copies result' '7' { Enter-Query }
    Set-Query '=1+2*3'
    Verify-Copy 'Calculator copy button copies result' '7' { [void][QuickInputWin]::Send($copyButton,0x00F5) }
    if($clipboardReady) {
        $held=@(0x10,0x11,0x12,0x5B,0x5C,0x43 | Where-Object { ([QuickInputWin]::GetAsyncKeyState($_) -band 0x8000) -ne 0 })
        if($held.Count -gt 0) { Skip 'Ctrl+Shift+C copies result' 'A modifier or C key is currently held; no keyboard input injected' }
        elseif(-not [QuickInputWin]::Focus($hostWindow,$edit)) { Skip 'Ctrl+Shift+C copies result' 'Could not focus the isolated input control; no keyboard input injected' }
        else { Verify-Copy 'Ctrl+Shift+C copies result' '7' { [QuickInputWin]::CtrlShiftC() } }
    } else { Skip 'Ctrl+Shift+C copies result' $clipboardSkipReason }

    Set-Query '=1/0'
    Wait-For { (Output-Text).Trim().Length -gt 0 -and -not [QuickInputWin]::IsWindowEnabled($copyButton) } 'invalid calculation disables copy'
    Check $true 'Invalid calculation shows an error and cannot be copied' ([ordered]@{output=(Output-Text);metadata=(Metadata-Text)})
    if($clipboardReady) {
        $sentinel='invalid calculator sentinel ' + [Guid]::NewGuid().ToString('N'); $clipboardTouched=$true
        Invoke-ClipboardWrite { [Windows.Forms.Clipboard]::SetText($sentinel) }
        Enter-Query; Start-Sleep -Milliseconds 150
        Wait-For { Clipboard-Matches $sentinel } 'Invalid calculation Enter leaves clipboard intact' 3000
        Check $true 'Invalid calculation Enter leaves clipboard intact'
    } else { Skip 'Invalid calculation Enter leaves clipboard intact' $clipboardSkipReason }
    Snapshot 'calculator-error.png'

    Set-Query ">Write-Output '中文测试'"
    Remains-True { -not (Output-Text).Contains('中文测试') -and (Metadata-Text) -notmatch '正在执行' -and -not [QuickInputWin]::IsWindowEnabled($copyButton) } 'Typing a PowerShell command does not execute it'
    Enter-Query; Wait-Command '中文测试'
    Check ((Normalize-Output (Output-Text)) -eq '中文测试') 'PowerShell captures Unicode stdout'
    Snapshot 'command-unicode.png'
    Verify-Copy 'Command copy button copies only output body' (Output-Text) { [void][QuickInputWin]::Send($copyButton,0x00F5) }
    Set-Query ">Write-Output '第一行'; Write-Output '第二行'"
    Enter-Query; Wait-Command '第二行'
    Check ((Normalize-Output (Output-Text)) -eq "第一行`n第二行") 'PowerShell preserves multiple output lines'
    Snapshot 'command-multiline.png'
    Set-Query '>1..24 | ForEach-Object { "line-$_-" + (''x'' * 100) }'
    Enter-Query; Wait-Command 'line-24-'
    $expectedLongOutput=@(1..24 | ForEach-Object { 'line-' + $_ + '-' + ('x' * 100) }) -join "`n"
    $longOutput=Output-Text
    Check ((Normalize-Output $longOutput) -eq $expectedLongOutput) 'Long command output retains all 24 complete lines in the result EDIT' ([ordered]@{lines=24;characters=$longOutput.Length})
    $longStyle=[QuickInputWin]::GetWindowLongPtr($resultEdit,-16).ToInt64()
    Check (($longStyle -band 0x0800) -ne 0 -and ($longStyle -band 0x00100000) -ne 0 -and ($longStyle -band 0x00200000) -ne 0) 'Long result remains read-only and enables horizontal and vertical scrollbars' ([ordered]@{style=$longStyle})
    Snapshot 'command-long-output.png'
    Verify-Copy 'Long command output copies the complete output body' $longOutput { [void][QuickInputWin]::Send($copyButton,0x00F5) }
    Set-Query ">[Console]::Error.WriteLine('错误哨兵'); exit 7"
    Enter-Query; Wait-Command '错误哨兵' 7
    Check ((Output-Text).Contains('错误哨兵')) 'PowerShell captures stderr and reports nonzero exit code' ([ordered]@{output=(Output-Text);metadata=(Metadata-Text)})
    Snapshot 'command-error.png'

    $repeatFile=Join-Path $runRoot 'repeat-fixture.txt'; $repeatLiteral=$repeatFile.Replace("'","''")
    Set-Query (">Add-Content -LiteralPath '$repeatLiteral' -Value 'run'; Start-Sleep -Milliseconds 800; Write-Output 'repeat-complete'")
    Enter-Query
    Wait-For { (Metadata-Text) -match '正在执行' -and -not [QuickInputWin]::IsWindowEnabled($copyButton) } 'command execution starts and copy is disabled'
    Enter-Query; Enter-Query; Enter-Query
    Wait-Command 'repeat-complete'
    $runs=@(Get-Content -LiteralPath $repeatFile)
    Check ($runs.Count -eq 1 -and $runs[0] -eq 'run') 'Repeated Enter during execution starts only one PowerShell process' ([ordered]@{fixture=$repeatFile;lines=$runs})
    [void][QuickInputWin]::PostMessage($edit,0x0100,[IntPtr]0x0D,[IntPtr]0x40000000)
    Remains-True {
        @(Get-Content -LiteralPath $repeatFile).Count -eq 1 -and (Metadata-Text) -notmatch '正在执行' -and [QuickInputWin]::IsWindowEnabled($copyButton)
    } 'Enter autorepeat with bit 30 cannot rerun a completed command' 800
    Enter-Query
    Wait-For { (Metadata-Text) -match '正在执行' } 'a fresh Enter press can rerun the completed command'
    Wait-Command 'repeat-complete'
    $runs=@(Get-Content -LiteralPath $repeatFile)
    Check ($runs.Count -eq 2 -and @($runs | Where-Object { $_ -ne 'run' }).Count -eq 0) 'A fresh Enter with lParam zero runs the command again' ([ordered]@{fixture=$repeatFile;lines=$runs})

    Set-Query ">Start-Sleep -Milliseconds 1200; Write-Output 'stale-calculator-output'"
    Enter-Query; Wait-For { (Metadata-Text) -match '正在执行' } 'old command starts before calculator query'
    Set-Query '=6*7'; Wait-For { (Output-Text).Trim() -eq '42' } 'new calculator result appears'
    Remains-True { (Output-Text).Trim() -eq '42' -and [QuickInputWin]::IsWindowEnabled($copyButton) } 'Old asynchronous command output cannot overwrite a new calculation' 2200
    Snapshot 'calculator-after-command.png'
    Wait-For { Test-Path -LiteralPath (Join-Path $dataRoot 'index.json') } 'application index exists'
    Set-Query ">Start-Sleep -Milliseconds 1200; Write-Output 'stale-search-output'"
    Enter-Query; Wait-For { (Metadata-Text) -match '正在执行' } 'old command starts before application search'
    Set-Query 'sbglq'
    Wait-For { [QuickInputWin]::IsWindowVisible($list) -and [QuickInputWin]::Send($list,0x018B) -gt 0 -and [QuickInputWin]::ListTitle($list,0) -eq '设备管理器' } 'application search returns after leaving command mode'
    Remains-True { [QuickInputWin]::IsWindowVisible($list) -and -not [QuickInputWin]::IsWindowVisible($resultEdit) -and [QuickInputWin]::ListTitle($list,0) -eq '设备管理器' } 'Old command completion cannot restore its result panel over application search' 2200
    Snapshot 'application-search-after-command.png'

    Set-Query '='
    Check ([QuickInputWin]::Text($edit) -eq '' -and [QuickInputWin]::IsWindowVisible($resultEdit) -and (Metadata-Text) -match '计算') 'Bare calculator trigger opens the mode with an empty input body'
    Snapshot 'calculator-mode-empty.png'
    Set-InputBody '2*(3+4)'; Wait-For { (Output-Text).Trim() -eq '14' } 'calculator updates from a body without a trigger'
    Check ([QuickInputWin]::Text($edit) -eq '2*(3+4)') 'Calculator body accepts expressions without a prefix'
    Delete-InputBody
    Check ([QuickInputWin]::Text($edit) -eq '' -and [QuickInputWin]::IsWindowVisible($resultEdit)) 'Deleting the whole calculator body keeps calculator mode open'
    Press-InputKey 0x08 -Character; Wait-RestoredSearch 'sbglq'
    Check $true 'Backspace on an empty calculator body restores the previous visible search'
    Press-InputKey 0x08 0x40000000 -Character
    Remains-True { [QuickInputWin]::IsWindowVisible($hostWindow) -and [QuickInputWin]::Text($edit) -eq 'sbglq' -and -not [QuickInputWin]::IsWindowVisible($resultEdit) } 'Backspace autorepeat cannot delete the restored search query' 350
    Set-Query '='; Press-InputKey 0x08
    [void][QuickInputWin]::PostMessage($edit,0x0102,[IntPtr]0x7F,[IntPtr]::Zero)
    Wait-RestoredSearch 'sbglq'
    Remains-True { [QuickInputWin]::IsWindowVisible($hostWindow) -and [QuickInputWin]::Text($edit) -eq 'sbglq' -and -not [QuickInputWin]::IsWindowVisible($resultEdit) } 'Ctrl+Backspace translated character cannot delete the restored search query' 350
    Set-Query '='; Press-InputKey 0x2E; Wait-RestoredSearch 'sbglq'
    Check $true 'Delete on an empty calculator body restores the previous search'
    Set-Query '=8*8'; Press-InputKey 0x1B -Character; Wait-RestoredSearch 'sbglq'
    Check $true 'Escape from a nonempty calculator body restores search without hiding the window'
    Press-InputKey 0x1B 0x40000000 -Character
    Remains-True { [QuickInputWin]::IsWindowVisible($hostWindow) -and [QuickInputWin]::Text($edit) -eq 'sbglq' } 'Escape autorepeat cannot hide the restored search window' 350
    Press-InputKey 0x1B -Character; Wait-For { -not [QuickInputWin]::IsWindowVisible($hostWindow) } 'a fresh Escape in search hides the launcher'
    Check $true 'A fresh Escape after returning to search hides the launcher'

    Set-Query '插件'; Set-Query '>'
    Check ([QuickInputWin]::Text($edit) -eq '' -and [QuickInputWin]::IsWindowVisible($resultEdit) -and (Metadata-Text) -match 'PowerShell') 'Bare terminal trigger opens the mode with an empty input body'
    Snapshot 'terminal-mode-empty.png'
    Set-InputBody ">Write-Output 'prefix-retained'"
    Check ([QuickInputWin]::Text($edit) -eq ">Write-Output 'prefix-retained'" -and (Metadata-Text) -match 'PowerShell') 'A terminal body beginning with greater-than remains literal terminal input'
    Set-InputBody '=1+2'
    Check ([QuickInputWin]::Text($edit) -eq '=1+2' -and (Metadata-Text) -match 'PowerShell' -and -not [QuickInputWin]::IsWindowEnabled($copyButton)) 'A terminal body beginning with equals does not switch to calculator mode'
    Set-InputBody "Write-Output '正文命令'"
    Enter-Query; Wait-Command '正文命令'
    Check ((Normalize-Output (Output-Text)) -eq '正文命令') 'Terminal executes a command body without a trigger prefix'
    Delete-InputBody
    Check ([QuickInputWin]::Text($edit) -eq '' -and [QuickInputWin]::IsWindowVisible($resultEdit)) 'Deleting the whole terminal body keeps terminal mode open'
    Set-InputBody "Write-Output 'escape-body'"; Press-InputKey 0x1B -Character; Wait-RestoredSearch '插件'
    Check $true 'Escape from a nonempty terminal body restores its previous search query'
    Set-Query '>'; Press-InputKey 0x08 -Character; Wait-RestoredSearch '插件'
    Check $true 'Backspace on an empty terminal body restores its previous search'
    Set-Query '>'; Press-InputKey 0x2E; Wait-RestoredSearch '插件'
    Check $true 'Delete on an empty terminal body restores its previous search'
    Set-Query ">Start-Sleep -Milliseconds 1200; Write-Output 'stale-exited-mode-output'"
    Enter-Query; Wait-For { (Metadata-Text) -match '正在执行' } 'command starts before leaving terminal mode'
    Press-InputKey 0x1B -Character; Wait-RestoredSearch '插件'
    Remains-True { [QuickInputWin]::IsWindowVisible($hostWindow) -and [QuickInputWin]::IsWindowVisible($list) -and -not [QuickInputWin]::IsWindowVisible($resultEdit) -and [QuickInputWin]::Text($edit) -eq '插件' } 'Old command completion cannot revive an exited mode or replace the restored query' 2200
    Snapshot 'command-exit-restored-search.png'
    Set-Query '退出'; Enter-Query
    Check ($gui.WaitForExit(5000)) 'Isolated host exits normally'
    $results.status=if($results.skipped.Count -gt 0) { 'passed_with_skips' } else { 'passed' }
} catch { $results.status='failed'; $results.failure=$_.Exception.Message; throw }
finally {
    if($gui -and -not $gui.HasExited) {
        try { Set-Query '退出'; Enter-Query; [void]$gui.WaitForExit(2000) } catch { $results.host_cleanup_error=$_.Exception.Message }
        if(-not $gui.HasExited) { $gui.Kill(); [void]$gui.WaitForExit(2000); $results.host_cleanup='Killed only the isolated host after normal exit failed' }
    }
    if($clipboardReady -and $clipboardTouched) {
        try {
            Invoke-ClipboardWrite {
                if($clipboardWasEmpty) { [Windows.Forms.Clipboard]::Clear() } else { [Windows.Forms.Clipboard]::SetDataObject($clipboardSnapshot,$true) }
            }
            $results.clipboard_restored=$true
        } catch { $results.clipboard_restored=$false; $results.clipboard_restore_error=$_.Exception.Message; $results.status='failed' }
    }
    foreach($copy in $clipboardCopies) { $copy.Dispose() }
    $results.foreground_restored=[QuickInputWin]::Focus($originalForeground,[IntPtr]::Zero)
    $results | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $runRoot 'results.json') -Encoding utf8NoBOM
    Write-Output "Evidence: $runRoot"
    if($results.clipboard_restored -eq $false) { throw 'Could not restore the original clipboard; see results.json' }
}
