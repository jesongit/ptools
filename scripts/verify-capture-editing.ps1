param([string]$Executable='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $Executable) { $Executable=Join-Path $projectRoot 'dist/ptools/plugins/capture/ptools-capture.exe' }
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$runRoot=Join-Path $projectRoot ('artifacts/capture-editing-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8))
$dataRoot=Join-Path $runRoot 'data'
$images=Join-Path $dataRoot 'images'
New-Item -ItemType Directory -Path $images -Force | Out-Null
$results=[ordered]@{timestamp=(Get-Date).ToString('o');executable=$Executable;binary_sha256=(Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash;data_dir=$dataRoot;status='running';checks=@();clipboard_restored=$null;input='Native mouse messages and SendInput keyboard; private fixture; inline editor and independent automatic OCR';clipboard_read='Native CF_UNICODETEXT and immediate CF_DIBV5; original formats restored through materialized snapshot';overlay_ocr='Real capture of our rendered fixture pin with automatic region OCR';limits=@('Live Pinyin candidate selection is not exercised; Chinese characters use Unicode window messages','Current monitor DPI and zoom are exercised; cross-monitor DPI changes are not exercised')}
$gui=$null; $overlayGui=$null; $clipboardWindow=$null; $clipboardReady=$false; $clipboardTouched=$false; $originalForeground=[IntPtr]::Zero; $originalDpi=[IntPtr]::Zero; $originalCursor=$null
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
public static class CaptureEditingWin {
  public delegate bool EnumProc(IntPtr h,IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] private static extern int GetClassName(IntPtr h,StringBuilder name,int max);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr h,StringBuilder name,int max);
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
  [DllImport("user32.dll")] public static extern IntPtr GetClipboardOwner();
  [DllImport("user32.dll")] private static extern bool OpenClipboard(IntPtr h);
  [DllImport("user32.dll")] private static extern bool EmptyClipboard();
  [DllImport("user32.dll")] private static extern bool CloseClipboard();
  [DllImport("user32.dll")] private static extern IntPtr SetClipboardData(uint format,IntPtr memory);
  [DllImport("user32.dll")] private static extern IntPtr GetClipboardData(uint format);
  [DllImport("user32.dll")] public static extern bool IsClipboardFormatAvailable(uint format);
  [DllImport("kernel32.dll")] private static extern IntPtr GlobalAlloc(uint flags,UIntPtr bytes);
  [DllImport("kernel32.dll")] private static extern IntPtr GlobalLock(IntPtr memory);
  [DllImport("kernel32.dll")] private static extern bool GlobalUnlock(IntPtr memory);
  [DllImport("kernel32.dll")] private static extern IntPtr GlobalFree(IntPtr memory);
  [DllImport("kernel32.dll")] private static extern UIntPtr GlobalSize(IntPtr memory);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint flags);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [DllImport("user32.dll")] private static extern bool GetGUIThreadInfo(uint thread,out GUI info);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW",CharSet=CharSet.Unicode)] private static extern IntPtr SendTextRaw(IntPtr h,uint msg,IntPtr w,string l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW",CharSet=CharSet.Unicode)] private static extern IntPtr ReadTextRaw(IntPtr h,uint msg,IntPtr w,StringBuilder l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] private static extern bool AttachThreadInput(uint from,uint to,bool attach);
  [DllImport("user32.dll")] private static extern IntPtr SetFocus(IntPtr h);
  [DllImport("user32.dll")] private static extern uint SendInput(uint count,INPUT[] inputs,int size);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
  [StructLayout(LayoutKind.Sequential)] public struct GUI { public uint Size,Flags;public IntPtr Active,Focus,Capture,MenuOwner,MoveSize,Caret;public RECT CaretRect; }
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
  public static string Describe(IntPtr h) {
    var name=new StringBuilder(256);var title=new StringBuilder(1024);GetClassName(h,name,256);GetWindowText(h,title,1024);
    return h.ToInt64()+": "+name+" / "+title;
  }
  public static long Send(IntPtr h,uint msg,long w=0,long l=0) {
    if(SendRaw(h,msg,new IntPtr(w),new IntPtr(l),2,2000,out UIntPtr result)==IntPtr.Zero) throw new Exception("Window message failed or timed out: "+msg);
    return unchecked((long)result.ToUInt64());
  }
  public static void Text(IntPtr h,string value) {
    if(SendTextRaw(h,0x000C,IntPtr.Zero,value,2,2000,out UIntPtr result)==IntPtr.Zero) throw new Exception("WM_SETTEXT failed or timed out");
  }
  public static void ClipboardText(IntPtr owner,string value) {
    byte[] bytes=Encoding.Unicode.GetBytes(value+"\0");IntPtr memory=GlobalAlloc(2,(UIntPtr)bytes.Length);
    if(memory==IntPtr.Zero) throw new Exception("Clipboard allocation failed");
    try {
      IntPtr data=GlobalLock(memory);if(data==IntPtr.Zero) throw new Exception("Clipboard lock failed");
      try { Marshal.Copy(bytes,0,data,bytes.Length); } finally { GlobalUnlock(memory); }
      if(!OpenClipboard(owner)) throw new ExternalException("Clipboard occupied");
      try { EmptyClipboard();if(SetClipboardData(13,memory)==IntPtr.Zero) throw new ExternalException("Clipboard write failed");memory=IntPtr.Zero; }
      finally { CloseClipboard(); }
    } finally { if(memory!=IntPtr.Zero) GlobalFree(memory); }
  }
  public static string ReadClipboardText() {
    if(!OpenClipboard(IntPtr.Zero)) throw new ExternalException("Clipboard occupied");
    try {
      IntPtr memory=GetClipboardData(13);if(memory==IntPtr.Zero) return "";
      IntPtr data=GlobalLock(memory);if(data==IntPtr.Zero) return "";
      try { return Marshal.PtrToStringUni(data)??""; } finally { GlobalUnlock(memory); }
    } finally { CloseClipboard(); }
  }
  public static byte[] ReadClipboardImage() {
    if(!OpenClipboard(IntPtr.Zero)) throw new ExternalException("Clipboard occupied");
    try {
      IntPtr memory=GetClipboardData(17);if(memory==IntPtr.Zero) throw new ExternalException("Native CF_DIBV5 image no longer available");
      ulong size=GlobalSize(memory).ToUInt64();if(size<124||size>128*1024*1024) throw new Exception("Unexpected CF_DIBV5 size");
      IntPtr data=GlobalLock(memory);if(data==IntPtr.Zero) throw new ExternalException("Clipboard image lock failed");
      try { byte[] bytes=new byte[(int)size];Marshal.Copy(data,bytes,0,bytes.Length);return bytes; } finally { GlobalUnlock(memory); }
    } finally { CloseClipboard(); }
  }
  public static byte[] CopyAndReadImage(IntPtr owner) {
    Send(owner,0x0111,9);
    var watch=System.Diagnostics.Stopwatch.StartNew();
    while(true) {
      try { return ReadClipboardImage(); }
      catch(ExternalException e) {
        if(e.Message!="Clipboard occupied"||watch.ElapsedMilliseconds>=3000) throw;
        System.Threading.Thread.Sleep(1);
      }
    }
  }
  public static bool Focus(IntPtr h) {
    if(GetForegroundWindow()==GetAncestor(h,2)&&FocusedWindow(h)==h) return true;
    uint current=GetCurrentThreadId(), foreground=GetWindowThreadProcessId(GetForegroundWindow(),out uint first), destination=GetWindowThreadProcessId(h,out uint second);
    bool a=current!=foreground && AttachThreadInput(current,foreground,true), b=current!=destination && destination!=foreground && AttachThreadInput(current,destination,true);
    try { IntPtr root=GetAncestor(h,2);if(GetForegroundWindow()!=root) SetForegroundWindow(root);SetFocus(h);return GetForegroundWindow()==root; }
    finally { if(b) AttachThreadInput(current,destination,false); if(a) AttachThreadInput(current,foreground,false); }
  }
  public static IntPtr FocusedWindow(IntPtr h) {
    uint thread=GetWindowThreadProcessId(h,out uint pid);var info=new GUI{Size=(uint)Marshal.SizeOf<GUI>()};
    return GetGUIThreadInfo(thread,out info)?info.Focus:IntPtr.Zero;
  }
  public static string TextValue(IntPtr h) {
    var value=new StringBuilder(4096);
    if(ReadTextRaw(h,0x000D,new IntPtr(value.Capacity),value,2,2000,out UIntPtr result)==IntPtr.Zero) throw new Exception("Cross-process WM_GETTEXT failed or timed out");
    return value.ToString();
  }
  public static void Key(ushort vk,bool ctrl=false,bool shift=false) {
    if(ctrl&&shift) throw new Exception("Combined modifiers are unnecessary for this verification");
    ushort modifier=ctrl?(ushort)0x11:(ushort)0x10;
    ushort[] keys=ctrl||shift?new ushort[]{modifier,vk,vk,modifier}:new ushort[]{vk,vk};
    var input=new INPUT[keys.Length];
    for(int i=0;i<keys.Length;i++) input[i]=new INPUT{Type=1,Data=new UNION{Key=new KEY{Vk=keys[i],Flags=i<keys.Length/2?0u:2u}}};
    if(SendInput((uint)input.Length,input,Marshal.SizeOf<INPUT>())!=(uint)input.Length) {
      var release=new INPUT[ctrl||shift?2:1];
      release[0]=new INPUT{Type=1,Data=new UNION{Key=new KEY{Vk=vk,Flags=2}}};
      if(ctrl||shift) release[1]=new INPUT{Type=1,Data=new UNION{Key=new KEY{Vk=modifier,Flags=2}}};
      SendInput((uint)release.Length,release,Marshal.SizeOf<INPUT>()); throw new Exception("SendInput key failed");
    }
  }
  public static void Mouse(int x,int y,uint flags) {
    int left=GetSystemMetrics(76),top=GetSystemMetrics(77),width=GetSystemMetrics(78),height=GetSystemMetrics(79);
    var input=new INPUT[]{new INPUT{Type=0,Data=new UNION{Mouse=new MOUSE{
      X=(int)Math.Round((x-left)*65535.0/Math.Max(1,width-1)),Y=(int)Math.Round((y-top)*65535.0/Math.Max(1,height-1)),Flags=0x8000u|0x4000u|1u|flags}}}};
    if(SendInput(1,input,Marshal.SizeOf<INPUT>())!=1) throw new Exception("SendInput mouse failed");
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
    Invoke-ClipboardWrite { $script:clipboardReadText=[CaptureEditingWin]::ReadClipboardText() }
    $script:clipboardReadText.Replace("`r`n","`n")
}
function Clipboard-Image([byte[]]$Payload=$null) {
    if(-not $Payload) {
        Invoke-ClipboardWrite { $script:clipboardReadImage=[CaptureEditingWin]::ReadClipboardImage() }
        $Payload=$script:clipboardReadImage; $script:clipboardReadImage=$null
    }
    $header=[BitConverter]::ToInt32($payload,0); $width=[BitConverter]::ToInt32($payload,4); $height=[BitConverter]::ToInt32($payload,8)
    if($header -ne 124 -or $width -le 0 -or $height -eq 0 -or [BitConverter]::ToUInt16($payload,14) -ne 32 -or $payload.Length -lt $header+$width*4*[Math]::Abs($height)) { throw 'Unexpected plugin CF_DIBV5 layout' }
    $bitmap=[Drawing.Bitmap]::new($width,[Math]::Abs($height),[Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $bits=$null
    try {
        $bits=$bitmap.LockBits([Drawing.Rectangle]::new(0,0,$bitmap.Width,$bitmap.Height),[Drawing.Imaging.ImageLockMode]::WriteOnly,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        if($height -lt 0 -and $bits.Stride -eq $width*4) { [Runtime.InteropServices.Marshal]::Copy($payload,$header,$bits.Scan0,$width*4*[Math]::Abs($height)) }
        else {
            for($y=0;$y -lt $bitmap.Height;$y++) {
                $sourceY=if($height -lt 0) { $y } else { $bitmap.Height-1-$y }
                [Runtime.InteropServices.Marshal]::Copy($payload,($header+$sourceY*$width*4),[IntPtr]::Add($bits.Scan0,$y*$bits.Stride),$width*4)
            }
        }
        $bitmap.UnlockBits($bits); $bits=$null
        return $bitmap
    } catch { if($bits) { $bitmap.UnlockBits($bits) }; $bitmap.Dispose(); throw }
}
function Set-Sentinel {
    $script:clipboardTouched=$true
    $script:sentinel='capture selection sentinel ' + [Guid]::NewGuid().ToString('N')
    Invoke-ClipboardWrite { [CaptureEditingWin]::ClipboardText($script:clipboardWindow.Handle,$script:sentinel) }
    # Let clipboard viewers finish processing this setup write before the app
    # receives its copy command; these external listeners briefly hold the lock.
    Start-Sleep -Milliseconds 200
}
function Verify-Copy([string]$Name,[string]$Expected,[scriptblock]$Action) {
    Set-Sentinel
    & $Action
    Wait-For { (Clipboard-Text) -eq $Expected } $Name 5000
    Check $true $Name @{copied=Clipboard-Text}
    Check ([CaptureEditingWin]::IsWindow($script:pin) -and @([CaptureEditingWin]::Windows($gui.Id,'ptools.capture.view')).Count -eq 2) ($Name + ' keeps the original pin and history alive')
}
function Client-Rect([IntPtr]$Window) {
    $r=[CaptureEditingWin+RECT]::new()
    if(-not [CaptureEditingWin]::GetClientRect($Window,[ref]$r)) { throw 'GetClientRect failed' }
    $r
}
function Snapshot([IntPtr]$Window,[string]$Name) {
    $dpi=[CaptureEditingWin]::SetThreadDpiAwarenessContext([IntPtr](-4)); $bitmap=$null; $graphics=$null
    try {
        $r=[CaptureEditingWin+RECT]::new()
        if(-not [CaptureEditingWin]::GetWindowRect($Window,[ref]$r)) { throw 'GetWindowRect failed' }
        $bitmap=[Drawing.Bitmap]::new($r.Right-$r.Left,$r.Bottom-$r.Top); $graphics=[Drawing.Graphics]::FromImage($bitmap)
        [void][CaptureEditingWin]::RedrawWindow($Window,[IntPtr]::Zero,[IntPtr]::Zero,0x185)
        $dc=$graphics.GetHdc()
        try { if(-not [CaptureEditingWin]::PrintWindow($Window,$dc,2)) { throw 'PrintWindow failed' } } finally { $graphics.ReleaseHdc($dc) }
        $bitmap.Save((Join-Path $runRoot $Name),[Drawing.Imaging.ImageFormat]::Png)
    } finally {
        if($graphics) { $graphics.Dispose() }; if($bitmap) { $bitmap.Dispose() }
        [void][CaptureEditingWin]::SetThreadDpiAwarenessContext($dpi)
    }
}
function Mouse-Point([IntPtr]$Window,[uint32]$Message,[int]$X,[int]$Y,[long]$Flags=0) {
    [void][CaptureEditingWin]::Send($Window,$Message,$Flags,($X -band 0xffff) -bor (($Y -band 0xffff) -shl 16))
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
# These are the expected public toolbar actions. Ink checks exercise rendered icons;
# clicks on selection/copy/mosaic exercise the native hit targets as well.
$toolbarItems=@(0,1,3,4,6,7,21,22,12,25,13,14,24,15,19,20,16,10,11,101,100,9)
function Toolbar-Geometry {
    $r=Client-Rect $script:toolbar
    $scale=[int][CaptureEditingWin]::GetDpiForWindow($script:toolbar)
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
        if([CaptureEditingWin]::GetAsyncKeyState($modifier) -lt 0) { throw 'A modifier key is held; keyboard checks stopped to avoid interfering with user input' }
    }
    if(-not [CaptureEditingWin]::Focus($script:pin)) { throw 'Cannot focus our private pin for the keyboard check' }
    [CaptureEditingWin]::Key($Code,$Ctrl)
    Start-Sleep -Milliseconds 150
}
function Key-In([IntPtr]$Window,[ushort]$Code,[bool]$Ctrl=$false,[bool]$Shift=$false) {
    foreach($modifier in @(0x10,0x11,0x12,0x5b,0x5c)) {
        if([CaptureEditingWin]::GetAsyncKeyState($modifier) -lt 0) { throw 'A modifier key is held; refusing keyboard input' }
    }
    if([CaptureEditingWin]::GetForegroundWindow() -ne [CaptureEditingWin]::GetAncestor($Window,2) -or [CaptureEditingWin]::FocusedWindow($Window) -ne $Window) { throw 'Our private child is not focused; refusing keyboard input rather than committing it by activating the parent' }
    [CaptureEditingWin]::Key($Code,$Ctrl,$Shift)
    Start-Sleep -Milliseconds 150
}
function Window-Rect([IntPtr]$Window) {
    $r=[CaptureEditingWin+RECT]::new()
    if(-not [CaptureEditingWin]::GetWindowRect($Window,[ref]$r)) { throw 'GetWindowRect failed' }; $r
}
function Wait-Ocr([string]$Root) {
    # Automatic OCR is debounced by a 250 ms window timer.
    Start-Sleep -Milliseconds 400
    # A stale result can finish and immediately schedule the current generation.
    # Wait for a quiet second instead of accepting the gap between these workers.
    $watch=[Diagnostics.Stopwatch]::StartNew(); $quiet=[Diagnostics.Stopwatch]::StartNew()
    while($watch.ElapsedMilliseconds -lt 35000) {
        if(@(Get-ChildItem -LiteralPath (Join-Path $Root 'work') -Filter 'ocr-*.png' -ErrorAction SilentlyContinue).Count -gt 0) { $quiet.Restart() }
        elseif($quiet.ElapsedMilliseconds -ge 1000) { return }
        Start-Sleep -Milliseconds 40
    }
    throw 'Automatic OCR did not settle within 35 seconds'
}
function Copy-PinImage([string]$Name) {
    $script:clipboardTouched=$true; $before=[CaptureEditingWin]::GetClipboardSequenceNumber()
    $payload=[CaptureEditingWin]::CopyAndReadImage($script:pin)
    if([CaptureEditingWin]::GetClipboardSequenceNumber() -eq $before) { throw 'Image copy did not replace the clipboard' }
    $image=Clipboard-Image $payload
    try { $image.Save((Join-Path $runRoot $Name),[Drawing.Imaging.ImageFormat]::Png) } finally { $image.Dispose() }
}
function Changed-Pixels([string]$Before,[string]$After,[int[]]$Region) {
    $a=[Drawing.Bitmap]::new((Join-Path $runRoot $Before)); $b=[Drawing.Bitmap]::new((Join-Path $runRoot $After))
    try {
        if($a.Size -ne $b.Size) { throw 'Compared exported images have different dimensions' }
        $changed=0
        for($y=$Region[1];$y -lt $Region[3];$y++) { for($x=$Region[0];$x -lt $Region[2];$x++) {
            if($a.GetPixel($x,$y).ToArgb() -ne $b.GetPixel($x,$y).ToArgb()) { $changed++ }
        } }; $changed
    } finally { $a.Dispose(); $b.Dispose() }
}
function Word-FramePixels([string]$Name,[int[]]$Region) {
    $bitmap=[Drawing.Bitmap]::new((Join-Path $runRoot $Name))
    try {
        $count=0
        for($y=$Region[1];$y -lt $Region[3];$y++) { for($x=$Region[0];$x -lt $Region[2];$x++) {
            $color=$bitmap.GetPixel($x,$y)
            # The monochrome fixture has no colored pixels in a word. Its OCR
            # selection frame uses the application's orange accent.
            if($color.R-$color.G -gt 30 -and $color.G-$color.B -gt 30) { $count++ }
        } }; $count
    } finally { $bitmap.Dispose() }
}
function Verify-ClearedFrame([string]$Before,[string]$After,[int[]]$Region,[string]$Name) {
    $selected=Word-FramePixels $Before $Region; $cleared=Word-FramePixels $After $Region
    Check ($selected -gt 30 -and $cleared -eq 0) $Name @{selected_frame_pixels=$selected;remaining_frame_pixels=$cleared;word_region=$Region}
    $actual=Clipboard-Text
    if($actual -ne $sentinel) {
        $script:results.clipboard_changes_during_deselection+=@{test=$Name;actual=$actual;sentinel=$sentinel;owner=[CaptureEditingWin]::Describe([CaptureEditingWin]::GetClipboardOwner())}
    }
}
function Native-PinDrag([double]$X,[double]$Y,[int]$Dx,[int]$Dy) {
    foreach($key in @(0x01,0x02,0x10,0x11,0x12,0x5b,0x5c)) {
        if([CaptureEditingWin]::GetAsyncKeyState($key) -lt 0) { throw 'Mouse button or modifier held; refusing native mouse input' }
    }
    if(-not [CaptureEditingWin]::Focus($script:pin)) { throw 'Cannot focus our private pin for mouse input' }
    $p=Image-Point $X $Y; $r=Window-Rect $script:pin; $x=$r.Left+$p[0]; $y=$r.Top+$p[1]
    [CaptureEditingWin]::Mouse($x,$y,0)
    [CaptureEditingWin]::Mouse($x,$y,2)
    try {
        for($i=1;$i -le 8;$i++) { [CaptureEditingWin]::Mouse(($x+[int]($Dx*$i/8)),($y+[int]($Dy*$i/8)),0); Start-Sleep -Milliseconds 25 }
    } finally { [CaptureEditingWin]::Mouse(($x+$Dx),($y+$Dy),4) }
    Start-Sleep -Milliseconds 150
}
function Toolbar-Fill([string]$Name,[int]$Id) {
    $g=Toolbar-Geometry; $i=[Array]::IndexOf($toolbarItems,$Id)
    $bitmap=[Drawing.Bitmap]::new((Join-Path $runRoot $Name))
    try { $bitmap.GetPixel(($g.pad+($i%$g.columns)*$g.cell+4),($g.pad+[int][Math]::Floor($i/$g.columns)*$g.cell+4)).ToArgb() } finally { $bitmap.Dispose() }
}

try {
    if([Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') { throw 'Run with pwsh -STA -File scripts/verify-capture-editing.ps1; no clipboard was changed' }
    $originalForeground=[CaptureEditingWin]::GetForegroundWindow()
    $originalCursor=[CaptureEditingWin+POINT]::new(); [void][CaptureEditingWin]::GetCursorPos([ref]$originalCursor)
    $originalDpi=[CaptureEditingWin]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $sequence=[CaptureEditingWin]::GetClipboardSequenceNumber(); $source=[Windows.Forms.Clipboard]::GetDataObject()
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
    if([CaptureEditingWin]::GetClipboardSequenceNumber() -ne $sequence) { throw 'Clipboard changed while snapshotting; no clipboard was changed by this script' }
    $clipboardReady=$true; $results.clipboard_formats=$formats
    $results.clipboard_changes_during_deselection=@()
    $clipboardWindow=[Windows.Forms.NativeWindow]::new()
    $params=[Windows.Forms.CreateParams]::new(); $params.Caption='ptools private capture verification clipboard'; $params.Style=[int]0x80000000
    $clipboardWindow.CreateHandle($params)

    $bitmap=[Drawing.Bitmap]::new(960,420); $bitmap.SetResolution(96,96)
    $graphics=[Drawing.Graphics]::FromImage($bitmap); $font=[Drawing.Font]::new('Microsoft YaHei',32,[Drawing.FontStyle]::Regular,[Drawing.GraphicsUnit]::Pixel)
    try {
        $graphics.Clear([Drawing.Color]::White)
        $graphics.TextRenderingHint=[Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        $graphics.DrawString('中文选择测试 Hello ptools 12345',$font,[Drawing.Brushes]::Black,32,36)
        $graphics.DrawString('第二行 Second line 67890',$font,[Drawing.Brushes]::Black,32,116)
        $bitmap.Save((Join-Path $images '1-1.png'),[Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Save((Join-Path $images '1-1-thumb.png'),[Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Save((Join-Path $runRoot 'fixture.png'),[Drawing.Imaging.ImageFormat]::Png)
    } finally { $font.Dispose(); $graphics.Dispose(); $bitmap.Dispose() }
    @(@{id='1-1';created=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds();source='私有文本编辑与默认框选验收';width=960;height=420}) | ConvertTo-Json -AsArray | Set-Content -LiteralPath (Join-Path $images 'index.json') -Encoding utf8NoBOM
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
    Check ($recognition.text -match 'Hello' -and $recognition.text -match 'Second') 'Offline OCR recognizes the private two-line fixture' $recognition.text
    $results.ocr=$recognition
    $word=@($recognition.words | Where-Object { $_.text -eq 'Hello' })[0]
    Check ($null -ne $word -and $word.bounds[2] -gt 4 -and $word.bounds[3] -gt 0) 'Hello has real image coordinate bounds'
    $box=$word.bounds; $wordY=$box[1]+$box[3]/2

    Set-Sentinel
    $gui=Start-Process -FilePath $Executable -ArgumentList @('--action','history','--data-dir',('"'+$dataRoot+'"')) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $runRoot 'pin-process.stdout.log') -RedirectStandardError (Join-Path $runRoot 'pin-process.stderr.log') -PassThru
    $results.process_id=$gui.Id
    Wait-For { $script:history=@([CaptureEditingWin]::Windows($gui.Id,'ptools.capture.view'))[0]; $null -ne $script:history -and [CaptureEditingWin]::GetDlgItem($history,1) -ne [IntPtr]::Zero } 'private history window and list'
    Check ([CaptureEditingWin]::Send([CaptureEditingWin]::GetDlgItem($history,1),0x018B) -eq 1) 'Private history contains exactly the generated fixture'
    [void][CaptureEditingWin]::Send($history,0x0111,(1 -bor (2 -shl 16)))
    Wait-For { $script:pin=@([CaptureEditingWin]::Windows($gui.Id,'ptools.capture.view') | Where-Object { $_ -ne $history })[0]; $null -ne $script:pin } 'original pinned fixture'
    $results.pin_dpi=[CaptureEditingWin]::GetDpiForWindow($pin)
    Wait-Ocr $dataRoot
    $gui.Refresh(); $results.pin_process_exited=$gui.HasExited; if($gui.HasExited) { $results.pin_process_exit_code=$gui.ExitCode }
    Check ([CaptureEditingWin]::IsWindow($pin)) 'Pin remains alive after automatic background OCR' @{hwnd=$pin.ToInt64();process_exited=$gui.HasExited}
    Check ((Clipboard-Text) -eq $sentinel) 'Opening a pin recognizes text in the background without replacing the clipboard'
    Verify-Copy 'A pin selects and copies Hello without pressing the OCR icon' 'Hello' { Drag-Image ($box[0]+2) $wordY ($box[0]+$box[2]-2) $wordY }
    Key 0x20
    Wait-For { $script:toolbar=@([CaptureEditingWin]::Windows($gui.Id,'ptools.capture.toolbar'))[0]; $null -ne $script:toolbar } 'pin toolbar'
    # Space enters pin editing with Rectangle selected; explicitly choose Move.
    [void][CaptureEditingWin]::Send($pin,0x0111,0)
    Snapshot $pin 'pin-default-selected.png'; Snapshot $toolbar 'toolbar-default.png'
    Check ((Toolbar-Fill 'toolbar-default.png' 12) -eq (Toolbar-Fill 'toolbar-default.png' 0) -and (Toolbar-Fill 'toolbar-default.png' 12) -ne (Toolbar-Fill 'toolbar-default.png' 1)) 'The OCR switch starts highlighted together with the move tool'

    Toolbar-Click 1
    Verify-Copy 'Text remains selectable while the rectangle tool is active' 'Hello' { Drag-Image ($box[0]+2) $wordY ($box[0]+$box[2]-2) $wordY }
    Snapshot $toolbar 'toolbar-rectangle-and-ocr.png'
    Check ((Toolbar-Fill 'toolbar-rectangle-and-ocr.png' 12) -eq (Toolbar-Fill 'toolbar-rectangle-and-ocr.png' 1)) 'Rectangle and OCR switch remain highlighted simultaneously'
    Copy-PinImage 'before-rectangle.png'
    Toolbar-Click 12
    Snapshot $toolbar 'toolbar-ocr-off.png'
    Check ((Toolbar-Fill 'toolbar-ocr-off.png' 12) -ne (Toolbar-Fill 'toolbar-ocr-off.png' 1)) 'Turning OCR off leaves the rectangle tool active'
    Set-Sentinel
    Drag-Image ($box[0]+2) ($box[1]+2) ($box[0]+$box[2]-2) ($box[1]+$box[3]-2)
    Check ((Clipboard-Text) -eq $sentinel) 'OCR off lets the rectangle tool draw over recognized text without copying it' @{actual=Clipboard-Text;sentinel=$sentinel}
    Copy-PinImage 'rectangle-over-text.png'
    $changed=Changed-Pixels 'before-rectangle.png' 'rectangle-over-text.png' @([int]$box[0],[int]$box[1],[int]($box[0]+$box[2]),[int]($box[1]+$box[3]))
    Check ($changed -gt 20) 'The tool retained by the disabled OCR switch renders a rectangle' @{changed_pixels=$changed}
    Toolbar-Click 21
    Toolbar-Click 12
    Wait-Ocr $dataRoot
    Verify-Copy 'Re-enabling OCR restores text selection with the rectangle tool still active' 'Hello' { Drag-Image ($box[0]+2) $wordY ($box[0]+$box[2]-2) $wordY }
    $wordTopLeft=Image-Point ($box[0]-2) ($box[1]-2); $wordBottomRight=Image-Point ($box[0]+$box[2]+2) ($box[1]+$box[3]+2)
    $pinWordRegion=@($wordTopLeft[0],$wordTopLeft[1],$wordBottomRight[0],$wordBottomRight[1])
    Snapshot $pin 'pin-selected-before-blank-drawing.png'
    Set-Sentinel
    Drag-Image 120 250 420 360
    Toolbar-Click 25
    Snapshot $pin 'pin-after-blank-drawing.png'
    Verify-ClearedFrame 'pin-selected-before-blank-drawing.png' 'pin-after-blank-drawing.png' $pinWordRegion 'Drawing from blank image space clears the previous text selection'
    Copy-PinImage 'blank-rectangle.png'
    Check ((Changed-Pixels 'fixture.png' 'blank-rectangle.png' @(100,230,440,380)) -gt 100) 'An enabled OCR switch allows drawing on blank image space'
    Toolbar-Click 21
    [void][CaptureEditingWin]::Send($pin,0x0111,0)
    Wait-Ocr $dataRoot
    Verify-Copy 'Returning to move mode keeps text selection enabled' 'Hello' { Drag-Image ($box[0]+2) $wordY ($box[0]+$box[2]-2) $wordY }
    Snapshot $pin 'pin-selected-before-blank-move.png'
    $before=Window-Rect $pin
    Set-Sentinel
    Native-PinDrag 700 250 48 32
    $after=Window-Rect $pin
    Check ([Math]::Abs(($after.Left-$before.Left)-48) -le 2 -and [Math]::Abs(($after.Top-$before.Top)-32) -le 2) 'Dragging blank pin space moves the pin' @{before=@($before.Left,$before.Top);after=@($after.Left,$after.Top)}
    [void][CaptureEditingWin]::Send($pin,0x0111,25)
    Snapshot $pin 'pin-after-blank-move.png'
    Verify-ClearedFrame 'pin-selected-before-blank-move.png' 'pin-after-blank-move.png' $pinWordRegion 'Dragging blank pin space clears the previous text selection'
    Verify-Copy 'Text selection continues after moving the pin' 'Hello' { Drag-Image ($box[0]+2) $wordY ($box[0]+$box[2]-2) $wordY }
    $before=Client-Rect $pin
    [void][CaptureEditingWin]::Send($pin,0x020A,(-120 -shl 16))
    $after=Client-Rect $pin
    Check ($after.Right -lt $before.Right -and $after.Bottom -lt $before.Bottom) 'Pin zoom changes the displayed image scale'
    Verify-Copy 'Reverse text drag after zoom still uses original OCR coordinates' 'Hello' { Drag-Image ($box[0]+$box[2]-2) $wordY ($box[0]+2) $wordY }
    Snapshot $pin 'pin-selected-after-zoom.png'

    # Clear selection by a blank drag, then capture only the private pin pixels.
    Set-Sentinel; Native-PinDrag 700 250 0 0
    $pinRect=Window-Rect $pin; $workArea=[Windows.Forms.Screen]::FromHandle($pin).WorkingArea
    [void][CaptureEditingWin]::SetWindowPos($pin,[IntPtr](-1),$workArea.Left+24,$workArea.Top+24,$pinRect.Right-$pinRect.Left,$pinRect.Bottom-$pinRect.Top,0x10)
    [void][CaptureEditingWin]::RedrawWindow($pin,[IntPtr]::Zero,[IntPtr]::Zero,0x185)
    Start-Sleep -Milliseconds 200
    $pinRect=Window-Rect $pin; $pinClient=Client-Rect $pin
    $overlayData=Join-Path $runRoot 'overlay-data'; New-Item -ItemType Directory -Path $overlayData -Force | Out-Null
    @{include_cursor=$false} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $overlayData 'settings.json') -Encoding utf8NoBOM
    $overlayGui=Start-Process -FilePath $Executable -ArgumentList @('--action','capture','--data-dir',('"'+$overlayData+'"')) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $runRoot 'capture-process.stdout.log') -RedirectStandardError (Join-Path $runRoot 'capture-process.stderr.log') -PassThru
    Wait-For { $script:overlay=@([CaptureEditingWin]::Windows($overlayGui.Id,'ptools.capture.view'))[0]; $null -ne $script:overlay } 'real screenshot overlay'
    $overlayRect=Window-Rect $overlay
    $cropX=$pinRect.Left+2-$overlayRect.Left; $cropY=$pinRect.Top+2-$overlayRect.Top
    $cropWidth=$pinClient.Right-4; $cropHeight=$pinClient.Bottom-4
    Mouse-Point $overlay 0x0201 $cropX $cropY 1
    Mouse-Point $overlay 0x0200 ($cropX+$cropWidth) ($cropY+$cropHeight) 1
    Mouse-Point $overlay 0x0202 ($cropX+$cropWidth) ($cropY+$cropHeight)
    Set-Sentinel; Wait-Ocr $overlayData
    Check ((Clipboard-Text) -eq $sentinel) 'Selecting a screenshot region recognizes text automatically without copying the whole region'
    $screenLeft=[int][Math]::Round($cropX+($box[0]+2)*$cropWidth/960)
    $screenRight=[int][Math]::Round($cropX+($box[0]+$box[2]-2)*$cropWidth/960)
    $screenY=[int][Math]::Round($cropY+$wordY*$cropHeight/420)
    Mouse-Point $overlay 0x0201 $screenLeft $screenY 1
    Mouse-Point $overlay 0x0200 $screenRight $screenY 1
    Mouse-Point $overlay 0x0202 $screenRight $screenY
    Wait-For { (Clipboard-Text) -eq 'Hello' } 'automatic text selection in a real screenshot' 5000
    Check $true 'Screenshot text can be selected without pressing the OCR icon'
    Check (@([CaptureEditingWin]::Windows($overlayGui.Id,'ptools.capture.view')).Count -eq 1) 'Screenshot text selection keeps the original overlay open'
    Snapshot $overlay 'capture-default-selected.png'
    Set-Sentinel
    $blankX=[int]($cropX+700*$cropWidth/960); $blankY=[int]($cropY+250*$cropHeight/420)
    Mouse-Point $overlay 0x0201 $blankX $blankY 1
    Mouse-Point $overlay 0x0200 ($blankX+12) ($blankY+10) 1
    Mouse-Point $overlay 0x0202 ($blankX+12) ($blankY+10)
    [void][CaptureEditingWin]::Send($overlay,0x0111,25)
    Snapshot $overlay 'capture-region-after-blank-drag.png'
    $screenWordRegion=@([int][Math]::Floor($cropX+($box[0]-3)*$cropWidth/960),[int][Math]::Floor($cropY+($box[1]-3)*$cropHeight/420),[int][Math]::Ceiling($cropX+($box[0]+$box[2]+3)*$cropWidth/960),[int][Math]::Ceiling($cropY+($box[1]+$box[3]+3)*$cropHeight/420))
    Verify-ClearedFrame 'capture-default-selected.png' 'capture-region-after-blank-drag.png' $screenWordRegion 'Dragging blank screenshot space clears the previous text selection'
    # The moved region preserves its original dimensions when copied.
    [void][CaptureEditingWin]::Send($overlay,0x0111,9)
    Wait-For { [CaptureEditingWin]::IsClipboardFormatAvailable(17) } 'copy moved screenshot region' 5000
    $captured=Clipboard-Image
    try {
        Check ($captured.Width -eq $cropWidth -and $captured.Height -eq $cropHeight) 'Blank screenshot drag moves the region without changing its size' @{actual=@($captured.Width,$captured.Height);expected=@($cropWidth,$cropHeight)}
        $captured.Save((Join-Path $runRoot 'capture-moved-result.png'),[Drawing.Imaging.ImageFormat]::Png)
    } finally { $captured.Dispose() }
    Check ($overlayGui.WaitForExit(5000)) 'Completing the private screenshot exits its process'
    $overlayGui.Dispose(); $overlayGui=$null

    # Inline text edits use a native child Edit attached directly to the pin.
    Copy-PinImage 'before-inline.png'
    [void][CaptureEditingWin]::Send($pin,0x0111,6)
    if(-not [CaptureEditingWin]::Focus($pin)) { throw 'Cannot activate pin before creating an inline editor' }
    $at=Image-Point 96 250
    Mouse-Point $pin 0x0201 $at[0] $at[1] 1
    Mouse-Point $pin 0x0202 $at[0] $at[1]
    Wait-For { $script:editor=[CaptureEditingWin]::GetDlgItem($pin,230); $editor -ne [IntPtr]::Zero -and [CaptureEditingWin]::IsWindowVisible($editor) } 'native inline editor'
    Check ([CaptureEditingWin]::GetParent($editor) -eq $pin -and @([CaptureEditingWin]::Windows($gui.Id,'ptools.tool.form')).Count -eq 0) 'Text input appears as a child on the image without a separate dialog'
    $editRect=Window-Rect $editor; $pinRect=Window-Rect $pin
    Check ([Math]::Abs(($editRect.Left-$pinRect.Left)-$at[0]) -le 3 -and [Math]::Abs(($editRect.Top-$pinRect.Top)-$at[1]) -le 3) 'Inline editor follows the clicked image coordinates after zoom' @{click=$at;editor=@(($editRect.Left-$pinRect.Left),($editRect.Top-$pinRect.Top))}
    [CaptureEditingWin]::Text($editor,'inline text 54321')
    [void][CaptureEditingWin]::Send($editor,0x00B1,'inline text 54321'.Length,'inline text 54321'.Length)
    Key-In $editor 0x0d $false $true
    foreach($character in '第二行 text'.ToCharArray()) { [void][CaptureEditingWin]::Send($editor,0x0102,[int]$character) }
    $inputValue=[CaptureEditingWin]::TextValue($editor)
    Check ($inputValue.Replace("`r`n","`n") -eq "inline text 54321`n第二行 text") 'Shift+Enter inserts a newline in the inline editor' $inputValue
    $multilineRect=Window-Rect $editor
    Check ($multilineRect.Bottom-$multilineRect.Top -gt $editRect.Bottom-$editRect.Top) 'The inline input box grows to display both text lines' @{initial_height=$editRect.Bottom-$editRect.Top;multiline_height=$multilineRect.Bottom-$multilineRect.Top}
    Snapshot $pin 'pin-inline-editing.png'
    Key-In $editor 0x0d
    Wait-For { [CaptureEditingWin]::GetDlgItem($pin,230) -eq [IntPtr]::Zero } 'Enter commits inline text'
    Copy-PinImage 'inline-committed.png'
    $changed=Changed-Pixels 'before-inline.png' 'inline-committed.png' @(90,240,900,410)
    Check ($changed -gt 100) 'Enter commits multiline text to exported image pixels' @{changed_pixels=$changed}
    Check ((Changed-Pixels 'before-inline.png' 'inline-committed.png' @(90,279,900,325)) -gt 30) 'The second inline text line is present in the exported image'
    $export=[Drawing.Bitmap]::new((Join-Path $runRoot 'inline-committed.png'))
    try { Check ($export.Width -eq 960 -and $export.Height -eq 420) 'Inline text export preserves original image dimensions after zoom' } finally { $export.Dispose() }
    $cancelAt=Image-Point 500 300
    Mouse-Point $pin 0x0201 $cancelAt[0] $cancelAt[1] 1; Mouse-Point $pin 0x0202 $cancelAt[0] $cancelAt[1]
    Wait-For { $script:editor=[CaptureEditingWin]::GetDlgItem($pin,230); $editor -ne [IntPtr]::Zero } 'second inline editor'
    [CaptureEditingWin]::Text($editor,'cancelled text')
    Key-In $editor 0x1b
    Wait-For { [CaptureEditingWin]::GetDlgItem($pin,230) -eq [IntPtr]::Zero } 'Escape cancels inline text'
    Copy-PinImage 'inline-cancelled.png'
    Check ((Changed-Pixels 'inline-committed.png' 'inline-cancelled.png' @(0,0,960,420)) -eq 0) 'Escape cancels the new text without changing exported image pixels'
    [void][CaptureEditingWin]::Send($pin,0x0111,21)
    Copy-PinImage 'inline-undone.png'
    Check ((Changed-Pixels 'before-inline.png' 'inline-undone.png' @(0,0,960,420)) -eq 0) 'Undo removes the committed inline text as one annotation'
    [void][CaptureEditingWin]::Send($pin,0x0111,6)
    Mouse-Point $pin 0x0201 $at[0] $at[1] 1; Mouse-Point $pin 0x0202 $at[0] $at[1]
    Wait-For { $script:editor=[CaptureEditingWin]::GetDlgItem($pin,230); $editor -ne [IntPtr]::Zero } 'inline editor for toolbar commit'
    [CaptureEditingWin]::Text($editor,'toolbar commits text')
    Toolbar-Click 1
    Wait-For { [CaptureEditingWin]::GetDlgItem($pin,230) -eq [IntPtr]::Zero } 'changing toolbar tool commits inline text'
    Copy-PinImage 'inline-toolbar-committed.png'
    Check ((Changed-Pixels 'before-inline.png' 'inline-toolbar-committed.png' @(90,240,900,410)) -gt 100) 'Toolbar actions commit typed text before switching tools'
    Snapshot $pin 'pin-inline-final.png'
    [void][CaptureEditingWin]::PostMessage($pin,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    [void][CaptureEditingWin]::PostMessage($history,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    Check ($gui.WaitForExit(5000)) 'Closing our private pin and history exits its process'
    $results.status='passed'
} catch {
    $results.status='failed'; $results.error=$_.Exception.Message
    $results.clipboard_text_at_failure=Clipboard-Text
    $results.clipboard_owner_at_failure=[CaptureEditingWin]::Describe([CaptureEditingWin]::GetClipboardOwner())
    if($gui) {
        $gui.Refresh(); $results.pin_process_exited_at_failure=$gui.HasExited
        if($gui.HasExited) { $results.pin_process_exit_code=$gui.ExitCode }
        $results.pin_windows_at_failure=@([CaptureEditingWin]::Windows($gui.Id,'',$false) | ForEach-Object { [CaptureEditingWin]::Describe($_) })
        if($editor) {
            $results.inline_editor_alive_at_failure=[CaptureEditingWin]::IsWindow($editor)
            $results.focused_hwnd_at_failure=[CaptureEditingWin]::FocusedWindow($pin).ToInt64()
            if([CaptureEditingWin]::IsWindow($editor)) { $results.inline_editor_text_at_failure=[CaptureEditingWin]::TextValue($editor) }
        }
    }
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
    if($clipboardWindow) { $clipboardWindow.DestroyHandle() }
    if($originalCursor) { [void][CaptureEditingWin]::SetCursorPos($originalCursor.X,$originalCursor.Y) }
    if($originalForeground -ne [IntPtr]::Zero -and [CaptureEditingWin]::IsWindow($originalForeground)) { [void][CaptureEditingWin]::SetForegroundWindow($originalForeground) }
    if($originalDpi -ne [IntPtr]::Zero) { [void][CaptureEditingWin]::SetThreadDpiAwarenessContext($originalDpi) }
    $results | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $runRoot 'results.json') -Encoding utf8NoBOM
    Write-Output "Evidence: $runRoot"
    if($results.clipboard_restored -eq $false) { throw 'Could not restore the original clipboard; see results.json' }
}
