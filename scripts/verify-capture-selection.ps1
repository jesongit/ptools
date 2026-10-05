param([string]$Executable='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $Executable) { $Executable=Join-Path $projectRoot 'dist/ptools/plugins/capture/ptools-capture.exe' }
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$runRoot=Join-Path $projectRoot ('artifacts/capture-selection-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8))
$dataRoot=Join-Path $runRoot 'data'
$images=Join-Path $dataRoot 'images'
New-Item -ItemType Directory -Path $images -Force | Out-Null
$results=[ordered]@{timestamp=(Get-Date).ToString('o');executable=$Executable;data_dir=$dataRoot;status='running';checks=@();clipboard_restored=$null;input='Native mouse messages and SendInput keyboard; private history fixture';overlay_ocr='Real capture of our rendered fixture pin with original-window OCR and text drag'}
$gui=$null; $overlayGui=$null; $clipboardReady=$false; $clipboardTouched=$false; $originalForeground=[IntPtr]::Zero; $originalDpi=[IntPtr]::Zero
$clipboardCopies=[Collections.Generic.List[IDisposable]]::new()
function Check([bool]$Ok,[string]$Name,[object]$Evidence=$null) {
    $script:results.checks += [ordered]@{name=$Name;passed=$Ok;evidence=$Evidence}
    if(-not $Ok) { throw "FAILED: $Name" }; Write-Output "PASS: $Name"
}
function Wait-For([scriptblock]$Condition,[string]$Name,[int]$TimeoutMs=10000) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while($watch.ElapsedMilliseconds -lt $TimeoutMs) {
        if(& $Condition) { return }; Start-Sleep -Milliseconds 40
    }
    throw "Timeout after $TimeoutMs ms: $Name"
}
Add-Type -AssemblyName System.Drawing,System.Windows.Forms
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class CaptureSelectionWin {
  public delegate bool EnumProc(IntPtr h,IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] private static extern int GetClassName(IntPtr h,StringBuilder name,int max);
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h,int id);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW")] private static extern IntPtr SendRaw(IntPtr h,uint msg,IntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int width,int height,uint flags);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool RedrawWindow(IntPtr h,IntPtr rect,IntPtr region,uint flags);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
  [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
  [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] private static extern bool AttachThreadInput(uint from,uint to,bool attach);
  [DllImport("user32.dll")] private static extern IntPtr SetFocus(IntPtr h);
  [DllImport("user32.dll")] private static extern uint SendInput(uint count,INPUT[] inputs,int size);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint Type; public UNION Data; }
  [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public KEY Key; [FieldOffset(0)] public MOUSE Mouse; }
  [StructLayout(LayoutKind.Sequential)] public struct KEY { public ushort Vk,Scan; public uint Flags,Time; public UIntPtr Extra; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSE { public int X,Y; public uint Data,Flags,Time; public UIntPtr Extra; }
  public static IntPtr[] Windows(uint pid,string match,bool visible=true) {
    var result=new List<IntPtr>();
    EnumWindows((h,l)=>{GetWindowThreadProcessId(h,out uint found);var name=new StringBuilder(256);GetClassName(h,name,256);
      if(found==pid && (match=="" || name.ToString()==match) && (!visible || IsWindowVisible(h))) result.Add(h);return true;},IntPtr.Zero);
    return result.ToArray();
  }
  public static long Send(IntPtr h,uint msg,long w=0,long l=0) {
    if(SendRaw(h,msg,new IntPtr(w),new IntPtr(l),2,2000,out UIntPtr result)==IntPtr.Zero) throw new Exception("Window message failed or timed out: "+msg);
    return unchecked((long)result.ToUInt64());
  }
  public static bool Focus(IntPtr h) {
    uint current=GetCurrentThreadId(), foreground=GetWindowThreadProcessId(GetForegroundWindow(),out uint first), destination=GetWindowThreadProcessId(h,out uint second);
    bool a=current!=foreground && AttachThreadInput(current,foreground,true), b=current!=destination && destination!=foreground && AttachThreadInput(current,destination,true);
    try { SetForegroundWindow(h); SetFocus(h); return GetForegroundWindow()==h; }
    finally { if(b) AttachThreadInput(current,destination,false); if(a) AttachThreadInput(current,foreground,false); }
  }
  public static void Key(ushort vk,bool ctrl=false) {
    ushort[] keys=ctrl?new ushort[]{0x11,vk,vk,0x11}:new ushort[]{vk,vk};
    var input=new INPUT[keys.Length];
    for(int i=0;i<keys.Length;i++) input[i]=new INPUT{Type=1,Data=new UNION{Key=new KEY{Vk=keys[i],Flags=i<keys.Length/2?0u:2u}}};
    if(SendInput((uint)input.Length,input,Marshal.SizeOf<INPUT>())!=(uint)input.Length) {
      var release=new INPUT[ctrl?2:1];
      release[0]=new INPUT{Type=1,Data=new UNION{Key=new KEY{Vk=vk,Flags=2}}};
      if(ctrl) release[1]=new INPUT{Type=1,Data=new UNION{Key=new KEY{Vk=0x11,Flags=2}}};
      SendInput((uint)release.Length,release,Marshal.SizeOf<INPUT>()); throw new Exception("SendInput key failed");
    }
  }
}
'@
function Invoke-ClipboardWrite([scriptblock]$Action) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while($true) {
        try { & $Action; return }
        catch {
            if($_.Exception.GetBaseException() -isnot [Runtime.InteropServices.ExternalException] -or $watch.ElapsedMilliseconds -ge 3000) { throw }
        }
        Start-Sleep -Milliseconds 40
    }
}
function Clipboard-Text {
    try { [Windows.Forms.Clipboard]::GetText().Replace("`r`n","`n") }
    catch { if($_.Exception.GetBaseException() -isnot [Runtime.InteropServices.ExternalException]) { throw }; '' }
}
function Set-Sentinel {
    $script:clipboardTouched=$true
    $script:sentinel='capture selection sentinel ' + [Guid]::NewGuid().ToString('N')
    Invoke-ClipboardWrite { [Windows.Forms.Clipboard]::SetText($script:sentinel) }
}
function Verify-Copy([string]$Name,[string]$Expected,[scriptblock]$Action) {
    Set-Sentinel
    & $Action
    Wait-For { (Clipboard-Text) -eq $Expected } $Name 5000
    Check $true $Name @{copied=Clipboard-Text}
    Check ([CaptureSelectionWin]::IsWindow($script:pin) -and @([CaptureSelectionWin]::Windows($gui.Id,'ptools.capture.view')).Count -eq 2) ($Name + ' keeps the original pin and history alive')
}
function Client-Rect([IntPtr]$Window) {
    $r=[CaptureSelectionWin+RECT]::new()
    if(-not [CaptureSelectionWin]::GetClientRect($Window,[ref]$r)) { throw 'GetClientRect failed' }
    $r
}
function Snapshot([IntPtr]$Window,[string]$Name) {
    $dpi=[CaptureSelectionWin]::SetThreadDpiAwarenessContext([IntPtr](-4)); $bitmap=$null; $graphics=$null
    try {
        $r=[CaptureSelectionWin+RECT]::new()
        if(-not [CaptureSelectionWin]::GetWindowRect($Window,[ref]$r)) { throw 'GetWindowRect failed' }
        $bitmap=[Drawing.Bitmap]::new($r.Right-$r.Left,$r.Bottom-$r.Top); $graphics=[Drawing.Graphics]::FromImage($bitmap)
        [void][CaptureSelectionWin]::RedrawWindow($Window,[IntPtr]::Zero,[IntPtr]::Zero,0x185)
        $dc=$graphics.GetHdc()
        try { if(-not [CaptureSelectionWin]::PrintWindow($Window,$dc,2)) { throw 'PrintWindow failed' } } finally { $graphics.ReleaseHdc($dc) }
        $bitmap.Save((Join-Path $runRoot $Name),[Drawing.Imaging.ImageFormat]::Png)
    } finally {
        if($graphics) { $graphics.Dispose() }; if($bitmap) { $bitmap.Dispose() }
        [void][CaptureSelectionWin]::SetThreadDpiAwarenessContext($dpi)
    }
}
function Mouse-Point([IntPtr]$Window,[uint32]$Message,[int]$X,[int]$Y,[long]$Flags=0) {
    [void][CaptureSelectionWin]::Send($Window,$Message,$Flags,($X -band 0xffff) -bor (($Y -band 0xffff) -shl 16))
}
function Image-Point([double]$X,[double]$Y) {
    $r=Client-Rect $script:pin
    @([int][Math]::Round(2 + $X * ($r.Right-4)/960),[int][Math]::Round(2 + $Y * ($r.Bottom-4)/420))
}
function Drag-Image([double]$X1,[double]$Y1,[double]$X2,[double]$Y2) {
    $a=Image-Point $X1 $Y1; $b=Image-Point $X2 $Y2
    Mouse-Point $script:pin 0x0201 $a[0] $a[1] 1
    Mouse-Point $script:pin 0x0200 $b[0] $b[1] 1
    Mouse-Point $script:pin 0x0202 $b[0] $b[1]
}
function Wait-Ocr([string]$Root) {
    # Automatic OCR starts after the 250 ms debounce timer. Require a quiet
    # second so a stale worker cannot make the current generation look finished.
    Start-Sleep -Milliseconds 400
    $watch=[Diagnostics.Stopwatch]::StartNew(); $quiet=[Diagnostics.Stopwatch]::StartNew()
    while($watch.ElapsedMilliseconds -lt 35000) {
        if(@(Get-ChildItem -LiteralPath (Join-Path $Root 'work') -Filter 'ocr-*.png' -ErrorAction SilentlyContinue).Count -gt 0) { $quiet.Restart() }
        elseif($quiet.ElapsedMilliseconds -ge 1000) { return }
        Start-Sleep -Milliseconds 40
    }
    throw 'Automatic OCR did not settle within 35 seconds'
}
# These are the expected public toolbar actions. Ink checks exercise rendered icons;
# clicks on the text switch/copy/mosaic exercise the native hit targets as well.
$toolbarItems=@(0,1,3,4,6,7,21,22,12,25,13,14,24,15,19,20,16,10,11,101,100,9)
function Toolbar-Geometry {
    $r=Client-Rect $script:toolbar
    $scale=[int][CaptureSelectionWin]::GetDpiForWindow($script:toolbar)
    $cell=[int][Math]::Floor((38*$scale+48)/96); $pad=[int][Math]::Floor((8*$scale+48)/96)
    $columns=[Math]::Min($toolbarItems.Count,[Math]::Max(1,[int][Math]::Floor(($r.Right-2*$pad)/$cell)))
    $style=[int][Math]::Floor((24*$scale+48)/96)
    @{cell=$cell;pad=$pad;columns=$columns;rows=[int][Math]::Ceiling($toolbarItems.Count/$columns);style=$style;style_columns=[Math]::Max(1,[int][Math]::Floor(($r.Right-2*$pad)/$style))}
}
function Toolbar-Click([int]$Id) {
    $g=Toolbar-Geometry
    if($Id -eq 38) { $x=$g.pad+(8%$g.style_columns)*$g.style+[int]($g.style/2); $y=$g.pad+$g.rows*$g.cell+[int][Math]::Floor(8/$g.style_columns)*$g.style+[int]($g.style/2) }
    else {
        $i=[Array]::IndexOf($toolbarItems,$Id)
        if($i -lt 0) { throw "Unknown toolbar item $Id" }
        $x=$g.pad+($i%$g.columns)*$g.cell+[int]($g.cell/2); $y=$g.pad+[int][Math]::Floor($i/$g.columns)*$g.cell+[int]($g.cell/2)
    }
    Mouse-Point $script:toolbar 0x0201 $x $y 1
    Mouse-Point $script:toolbar 0x0202 $x $y
}
function Key([ushort]$Code,[bool]$Ctrl=$false) {
    foreach($modifier in @(0x10,0x11,0x12,0x5b,0x5c)) {
        if([CaptureSelectionWin]::GetAsyncKeyState($modifier) -lt 0) { throw 'A modifier key is held; keyboard checks stopped to avoid interfering with user input' }
    }
    if(-not [CaptureSelectionWin]::Focus($script:pin)) { throw 'Cannot focus our private pin for the keyboard check' }
    [CaptureSelectionWin]::Key($Code,$Ctrl)
    Start-Sleep -Milliseconds 150
}
try {
    if([Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') { throw 'Run with pwsh -STA -File scripts/verify-capture-selection.ps1; no clipboard was changed' }
    $originalForeground=[CaptureSelectionWin]::GetForegroundWindow()
    $originalDpi=[CaptureSelectionWin]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $sequence=[CaptureSelectionWin]::GetClipboardSequenceNumber(); $source=[Windows.Forms.Clipboard]::GetDataObject()
    $clipboardSnapshot=[Windows.Forms.DataObject]::new(); $clipboardWasEmpty=$null -eq $source
    $formats=@()
    if($source) {
        $formats=@($source.GetFormats($false)); $clipboardWasEmpty=$formats.Count -eq 0
        foreach($format in $formats) {
            $value=$source.GetData($format,$false)
            if($value -is [Drawing.Image]) { $value=$value.Clone(); $clipboardCopies.Add($value) }
            elseif($value -is [IO.Stream]) {
                if(-not $value.CanSeek) { throw "Cannot safely snapshot nonseekable clipboard stream: $format; no clipboard was changed" }
                $stream=[IO.MemoryStream]::new(); $position=$value.Position
                try { $value.Position=0; $value.CopyTo($stream) } finally { $value.Position=$position }
                $stream.Position=0; $value=$stream; $clipboardCopies.Add($value)
            } elseif($value -is [Array]) { $value=$value.Clone() }
            elseif($null -ne $value -and $value -isnot [string]) { throw "Unsupported clipboard format: $format; no clipboard was changed" }
            if($null -eq $value) { throw "Cannot materialize clipboard format: $format; no clipboard was changed" }
            $clipboardSnapshot.SetData($format,$false,$value)
        }
    }
    if([CaptureSelectionWin]::GetClipboardSequenceNumber() -ne $sequence) { throw 'Clipboard changed while snapshotting; no clipboard was changed by this script' }
    $clipboardReady=$true; $results.clipboard_formats=$formats

    $bitmap=[Drawing.Bitmap]::new(960,420); $bitmap.SetResolution(96,96)
    $graphics=[Drawing.Graphics]::FromImage($bitmap); $font=[Drawing.Font]::new('Microsoft YaHei',32,[Drawing.FontStyle]::Regular,[Drawing.GraphicsUnit]::Pixel)
    try {
        $graphics.Clear([Drawing.Color]::White)
        $graphics.TextRenderingHint=[Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        $graphics.DrawString('中文选择测试 Hello ptools 12345',$font,[Drawing.Brushes]::Black,32,36)
        $graphics.DrawString('第二行 Second line 67890',$font,[Drawing.Brushes]::Black,32,116)
        # A private colorful field makes path-only mosaic behavior measurable.
        for($y=240;$y -lt 400;$y++) { for($x=32;$x -lt 920;$x++) {
            $bitmap.SetPixel($x,$y,[Drawing.Color]::FromArgb(($x*19)%256,($y*23)%256,(($x+$y)*31)%256))
        } }
        $bitmap.Save((Join-Path $images '1-1.png'),[Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Save((Join-Path $images '1-1-thumb.png'),[Drawing.Imaging.ImageFormat]::Png)
    } finally { $font.Dispose(); $graphics.Dispose(); $bitmap.Dispose() }
    @(@{id='1-1';created=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds();source='私有中英文双行选择验收';width=960;height=420}) | ConvertTo-Json -AsArray | Set-Content -LiteralPath (Join-Path $images 'index.json') -Encoding utf8NoBOM
    $info=[Diagnostics.ProcessStartInfo]::new($Executable)
    $info.UseShellExecute=$false; $info.CreateNoWindow=$true; $info.WindowStyle=[Diagnostics.ProcessWindowStyle]::Hidden
    $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true
    $info.ArgumentList.Add('--ocr'); $info.ArgumentList.Add((Join-Path $images '1-1.png'))
    $probe=[Diagnostics.Process]::Start($info)
    try {
        $read=$probe.StandardOutput.ReadToEndAsync(); $errors=$probe.StandardError.ReadToEndAsync()
        if(-not $probe.WaitForExit(30000)) { $probe.Kill(); throw 'Fixture OCR timed out' }
        if($probe.ExitCode -ne 0) { throw "Fixture OCR failed: $($errors.GetAwaiter().GetResult())" }
        $recognition=$read.GetAwaiter().GetResult() | ConvertFrom-Json
    } finally { $probe.Dispose() }
    Check ($recognition.text -match 'Hello' -and $recognition.text -match 'Second' -and $recognition.text -match '12345' -and $recognition.text -match '67890') 'Offline OCR recognizes known English and numeric fixture on two lines' $recognition.text
    if($recognition.language -match '^zh') { Check ($recognition.text -match '中文' -and $recognition.text -match '第二行') 'Installed Chinese OCR recognizes known Chinese fixture' }
    $results.ocr=$recognition
    $word=@($recognition.words | Where-Object { $_.text -eq 'Hello' })[0]
    Check ($null -ne $word -and $word.bounds[2] -gt 4 -and $word.bounds[3] -gt 0) 'Hello has a real image coordinate word box'

    $gui=Start-Process -FilePath $Executable -ArgumentList @('--action','history','--data-dir',('"'+$dataRoot+'"')) -WindowStyle Hidden -PassThru
    $results.process_id=$gui.Id
    Wait-For { $script:history=@([CaptureSelectionWin]::Windows($gui.Id,'ptools.capture.view'))[0]; $null -ne $script:history } 'private history window'
    Check ([CaptureSelectionWin]::Send([CaptureSelectionWin]::GetDlgItem($history,1),0x018B) -eq 1) 'Private history contains exactly the generated fixture'
    Set-Sentinel
    [void][CaptureSelectionWin]::Send($history,0x0111,(1 -bor (2 -shl 16)))
    Wait-For { $script:pin=@([CaptureSelectionWin]::Windows($gui.Id,'ptools.capture.view') | Where-Object { $_ -ne $history })[0]; $null -ne $script:pin } 'original pinned fixture'
    Snapshot $pin 'pin-before-ocr.png'
    Key 0x20
    Wait-For { $script:toolbar=@([CaptureSelectionWin]::Windows($gui.Id,'ptools.capture.toolbar'))[0]; $null -ne $script:toolbar } 'direct icon toolbar'
    Snapshot $toolbar 'toolbar-all-icons.png'
    $g=Toolbar-Geometry; $toolbarBitmap=[Drawing.Bitmap]::new((Join-Path $runRoot 'toolbar-all-icons.png'))
    try {
        foreach($id in @(12,25,13,14,24,15,19,20,16,17,18,101)) {
            $i=[Array]::IndexOf($toolbarItems,$id); $left=$g.pad+($i%$g.columns)*$g.cell; $top=$g.pad+[int][Math]::Floor($i/$g.columns)*$g.cell
            $colors=[Collections.Generic.HashSet[int]]::new()
            for($y=$top+5;$y -lt $top+$g.cell-5;$y++) { for($x=$left+5;$x -lt $left+$g.cell-5;$x++) { [void]$colors.Add($toolbarBitmap.GetPixel($x,$y).ToArgb()) } }
            Check ($colors.Count -ge 2) "Action $id has an exposed, rendered toolbar icon" @{distinct_colors=$colors.Count}
        }
    } finally { $toolbarBitmap.Dispose() }
    Wait-Ocr $dataRoot
    Check ((Clipboard-Text) -eq $sentinel) 'Recognizing text does not automatically copy the whole image text'
    Check (@([CaptureSelectionWin]::Windows($gui.Id,'ptools.capture.view')).Count -eq 2 -and [CaptureSelectionWin]::IsWindow($pin)) 'OCR stays in the original pin without creating another pin'
    $box=$word.bounds
    Verify-Copy 'Dragging a word in the image copies it on mouse release with the default text switch' 'Hello' { Drag-Image ($box[0]+2) ($box[1]+$box[3]/2) ($box[0]+$box[2]-2) ($box[1]+$box[3]/2) }
    Snapshot $pin 'selected-hello.png'; Snapshot $toolbar 'toolbar-text-selection.png'
    Verify-Copy 'Exposed copy-text icon copies the current selection' 'Hello' { Toolbar-Click 25 }
    Verify-Copy 'Ctrl+C copies selected text' 'Hello' { Key 0x43 $true }
    Verify-Copy 'Enter copies selected text' 'Hello' { Key 0x0d }
    # Reverse direction and image/client mapping must remain correct after zoom.
    $before=Client-Rect $pin
    [void][CaptureSelectionWin]::Send($pin,0x020A,(-120 -shl 16))
    $after=Client-Rect $pin
    Check ($after.Right -lt $before.Right -and $after.Bottom -lt $before.Bottom) 'Mouse wheel changes the displayed image scale'
    Verify-Copy 'Reverse word drag uses original OCR coordinates after zoom' 'Hello' { Drag-Image ($box[0]+$box[2]-2) ($box[1]+$box[3]/2) ($box[0]+2) ($box[1]+$box[3]/2) }
    Snapshot $pin 'selected-hello-zoomed.png'
    Set-Sentinel; Key 0x41 $true
    Check ((Clipboard-Text) -eq $sentinel) 'Ctrl+A selects all text without replacing the clipboard'
    Toolbar-Click 25
    Wait-For { $script:allText=Clipboard-Text; $script:allText -match 'Hello' -and $script:allText -match 'Second' -and $script:allText -match '12345' -and $script:allText -match '67890' -and $script:allText.Contains("`n") } 'all selected text copied with line breaks' 5000
    Check $true 'Ctrl+A and copy-text icon preserve both text lines' $allText
    Snapshot $pin 'selected-all-lines.png'

    # Capture actual screen pixels of our pin. Removing selection highlights makes
    # the capture fixture independent of the previous text-selection rendering.
    Toolbar-Click 12
    $pinRect=[CaptureSelectionWin+RECT]::new(); [void][CaptureSelectionWin]::GetWindowRect($pin,[ref]$pinRect)
    $workArea=[Windows.Forms.Screen]::FromHandle($pin).WorkingArea
    [void][CaptureSelectionWin]::SetWindowPos($pin,[IntPtr](-1),$workArea.Left+24,$workArea.Top+24,$pinRect.Right-$pinRect.Left,$pinRect.Bottom-$pinRect.Top,0x10)
    [void][CaptureSelectionWin]::RedrawWindow($pin,[IntPtr]::Zero,[IntPtr]::Zero,0x185)
    Start-Sleep -Milliseconds 200
    [void][CaptureSelectionWin]::GetWindowRect($pin,[ref]$pinRect)
    $pinClient=Client-Rect $pin
    $overlayData=Join-Path $runRoot 'overlay-data'; New-Item -ItemType Directory -Path $overlayData -Force | Out-Null
    @{include_cursor=$false} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $overlayData 'settings.json') -Encoding utf8NoBOM
    $overlayGui=Start-Process -FilePath $Executable -ArgumentList @('--action','capture','--data-dir',('"'+$overlayData+'"')) -WindowStyle Hidden -PassThru
    Wait-For { $script:overlay=@([CaptureSelectionWin]::Windows($overlayGui.Id,'ptools.capture.view'))[0]; $null -ne $script:overlay } 'real screenshot overlay'
    $overlayRect=[CaptureSelectionWin+RECT]::new(); [void][CaptureSelectionWin]::GetWindowRect($overlay,[ref]$overlayRect)
    $cropX=$pinRect.Left+2-$overlayRect.Left; $cropY=$pinRect.Top+2-$overlayRect.Top
    $cropWidth=$pinClient.Right-4; $cropHeight=$pinClient.Bottom-4
    Set-Sentinel
    Mouse-Point $overlay 0x0201 $cropX $cropY 1
    Mouse-Point $overlay 0x0200 ($cropX+$cropWidth) ($cropY+$cropHeight) 1
    Mouse-Point $overlay 0x0202 ($cropX+$cropWidth) ($cropY+$cropHeight)
    Snapshot $overlay 'capture-fixture-selection.png'
    Wait-Ocr $overlayData
    Check ((Clipboard-Text) -eq $sentinel) 'Screenshot OCR does not automatically copy all recognized text'
    Check ([CaptureSelectionWin]::IsWindow($overlay) -and @([CaptureSelectionWin]::Windows($overlayGui.Id,'ptools.capture.view')).Count -eq 1) 'Screenshot OCR stays in the original overlay without creating a pin'
    $wordLeft=[int][Math]::Round($cropX+($box[0]+2)*$cropWidth/960)
    $wordRight=[int][Math]::Round($cropX+($box[0]+$box[2]-2)*$cropWidth/960)
    $wordY=[int][Math]::Round($cropY+($box[1]+$box[3]/2)*$cropHeight/420)
    Set-Sentinel
    Mouse-Point $overlay 0x0201 $wordLeft $wordY 1
    Mouse-Point $overlay 0x0200 $wordRight $wordY 1
    Mouse-Point $overlay 0x0202 $wordRight $wordY
    Wait-For { (Clipboard-Text) -eq 'Hello' } 'word selected directly in real screenshot copies Hello' 5000
    Check $true 'Dragging text directly in the screenshot copies the selected word' @{copied=Clipboard-Text;crop=@($cropX,$cropY,$cropWidth,$cropHeight);drag=@($wordLeft,$wordY,$wordRight,$wordY)}
    Check ([CaptureSelectionWin]::IsWindow($overlay) -and @([CaptureSelectionWin]::Windows($overlayGui.Id,'ptools.capture.view')).Count -eq 1) 'Screenshot word copy keeps the original overlay open'
    Snapshot $overlay 'capture-selected-hello.png'
    [void][CaptureSelectionWin]::PostMessage($overlay,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    Check ($overlayGui.WaitForExit(5000)) 'Closing the real screenshot overlay exits its private process'
    $overlayGui.Dispose(); $overlayGui=$null

    Toolbar-Click 38
    Drag-Image 180 300 780 300
    Snapshot $pin 'mosaic-pen-path.png'; Snapshot $toolbar 'toolbar-mosaic-color.png'
    Set-Sentinel; [void][CaptureSelectionWin]::Send($pin,0x0111,9)
    Wait-For { [Windows.Forms.Clipboard]::ContainsImage() } 'mosaic result copied as image' 5000
    $actual=[Windows.Forms.Clipboard]::GetImage(); $expected=[Drawing.Bitmap]::new((Join-Path $images '1-1.png'))
    try {
        Check ($actual.Width -eq 960 -and $actual.Height -eq 420) 'Copying the zoomed pin preserves original image dimensions'
        $changed=0; $awayChanged=0
        for($x=220;$x -lt 740;$x++) { if($actual.GetPixel($x,300).ToArgb() -ne $expected.GetPixel($x,300).ToArgb()) { $changed++ }; if($actual.GetPixel($x,360).ToArgb() -ne $expected.GetPixel($x,360).ToArgb()) { $awayChanged++ } }
        Check ($changed -gt 100 -and $awayChanged -eq 0) 'Mosaic color applies along the pen path and preserves pixels away from it' @{path_pixels_changed=$changed;away_pixels_changed=$awayChanged}
        $actual.Save((Join-Path $runRoot 'mosaic-result.png'),[Drawing.Imaging.ImageFormat]::Png)
    } finally { $actual.Dispose(); $expected.Dispose() }
    [void][CaptureSelectionWin]::PostMessage($pin,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    [void][CaptureSelectionWin]::PostMessage($history,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    Check ($gui.WaitForExit(5000)) 'Closing our last pin and history terminates the capture process'
    $results.status='passed'
} catch {
    $results.status='failed'; $results.error=$_.Exception.Message
    throw
} finally {
    if($overlayGui) {
        $overlayGui.Refresh()
        if(-not $overlayGui.HasExited) { $overlayGui.Kill($true); $overlayGui.WaitForExit() }
        $overlayGui.Dispose()
    }
    if($gui) {
        $gui.Refresh()
        if(-not $gui.HasExited) { $gui.Kill($true); $gui.WaitForExit() }
        $gui.Dispose()
    }
    if($clipboardReady -and $clipboardTouched) {
        try {
            Invoke-ClipboardWrite { if($clipboardWasEmpty) { [Windows.Forms.Clipboard]::Clear() } else { [Windows.Forms.Clipboard]::SetDataObject($clipboardSnapshot,$true) } }
            $results.clipboard_restored=$true
        } catch { $results.clipboard_restored=$false; $results.clipboard_restore_error=$_.Exception.Message; $results.status='failed' }
    }
    foreach($copy in $clipboardCopies) { $copy.Dispose() }
    if($originalForeground -ne [IntPtr]::Zero -and [CaptureSelectionWin]::IsWindow($originalForeground)) { [void][CaptureSelectionWin]::SetForegroundWindow($originalForeground) }
    if($originalDpi -ne [IntPtr]::Zero) { [void][CaptureSelectionWin]::SetThreadDpiAwarenessContext($originalDpi) }
    $results | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $runRoot 'results.json') -Encoding utf8NoBOM
    Write-Output "Evidence: $runRoot"
    if($results.clipboard_restored -eq $false) { throw 'Could not restore the original clipboard; see results.json' }
}
