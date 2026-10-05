param([string]$Executable='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $Executable) { $Executable=Join-Path $projectRoot 'dist/ptools/plugins/capture/ptools-capture.exe' }
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$runRoot=Join-Path $projectRoot ('artifacts/capture-groups-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8))
$dataRoot=Join-Path $runRoot 'data'
$images=Join-Path $dataRoot 'images'
New-Item -ItemType Directory -Path $images -Force | Out-Null
$results=[ordered]@{timestamp=(Get-Date).ToString('o');executable=$Executable;binary_sha256=(Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash;data_dir=$dataRoot;status='running';checks=@();clipboard_restored=$null;input='Native mouse messages and actual native popup mouse choices; SendInput for menu, Space and copy keys; bounded WM_KEYDOWN/UP for letter shortcut handling; private fixture';clipboard_read='Native CF_UNICODETEXT and immediate CF_DIBV5; original formats restored through materialized snapshot';overlay_ocr='Real capture of our rendered private pin; grouped tools in both surfaces';limits=@('Current monitor DPI is exercised; cross-monitor DPI changes are not exercised','Letter shortcuts are delivered directly to the private window because WeType intercepted a real R key during verification; physical letter shortcuts under Chinese IME are not verified')}
$gui=$null; $overlayGui=$null; $clipboardWindow=$null; $clipboardReady=$false; $clipboardTouched=$false; $originalForeground=[IntPtr]::Zero; $originalDpi=[IntPtr]::Zero; $originalCursor=$null
$clipboardCopies=[Collections.Generic.List[IDisposable]]::new()
function Check([bool]$Ok,[string]$Name,[object]$Evidence=$null) {
    $script:results.checks += [ordered]@{name=$Name;passed=$Ok;evidence=$Evidence}
    if(-not $Ok) { throw "FAILED: $Name" }; Write-Host "PASS: $Name"
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
public static class CaptureGroupsWin {
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
  [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr menu);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] private static extern bool GetMenuItemInfo(IntPtr menu,uint item,bool byPosition,ref MENUITEMINFO info);
  [DllImport("user32.dll")] public static extern bool GetMenuItemRect(IntPtr owner,IntPtr menu,uint item,out RECT rect);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW",CharSet=CharSet.Unicode)] private static extern IntPtr SendTextRaw(IntPtr h,uint msg,IntPtr w,string l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW",CharSet=CharSet.Unicode)] private static extern IntPtr ReadTextRaw(IntPtr h,uint msg,IntPtr w,StringBuilder l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] private static extern bool AttachThreadInput(uint from,uint to,bool attach);
  [DllImport("user32.dll")] private static extern IntPtr SetFocus(IntPtr h);
  [DllImport("user32.dll")] private static extern uint SendInput(uint count,INPUT[] inputs,int size);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
  [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct MENUITEMINFO {
    public uint Size,Mask,Type,State,Id;public IntPtr Submenu,Checked,Unchecked;public UIntPtr Data;public IntPtr Text;public uint Length;public IntPtr Bitmap;
  }
  public sealed class MenuRow { public uint Id,State,Type;public string Text;public RECT Bounds; }
  public static MenuRow[] MenuRows(IntPtr window,IntPtr menu) {
    int count=GetMenuItemCount(menu);if(count<0) throw new Exception("GetMenuItemCount failed");
    var rows=new MenuRow[count];
    for(uint i=0;i<count;i++) {
      IntPtr buffer=Marshal.AllocHGlobal(4096);
      try {
        var info=new MENUITEMINFO{Size=(uint)Marshal.SizeOf<MENUITEMINFO>(),Mask=0x1u|0x2u|0x40u|0x100u,Text=buffer,Length=2048};
        if(!GetMenuItemInfo(menu,i,true,ref info)) throw new Exception("GetMenuItemInfo failed");
        RECT rect;if(!GetMenuItemRect(IntPtr.Zero,menu,i,out rect)) throw new Exception("GetMenuItemRect failed");
        rows[i]=new MenuRow{Id=info.Id,State=info.State,Type=info.Type,Text=Marshal.PtrToStringUni(buffer)??"",Bounds=rect};
      } finally { Marshal.FreeHGlobal(buffer); }
    }
    return rows;
  }
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
    // Read on the same native call immediately after WM_COMMAND returns. Local
    // clipboard history software may restore an earlier format a moment later.
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
    Invoke-ClipboardWrite { $script:clipboardReadText=[CaptureGroupsWin]::ReadClipboardText() }
    $script:clipboardReadText.Replace("`r`n","`n")
}
function Clipboard-Image([byte[]]$Payload=$null) {
    if(-not $Payload) {
        Invoke-ClipboardWrite { $script:clipboardReadImage=[CaptureGroupsWin]::ReadClipboardImage() }
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
    Invoke-ClipboardWrite { [CaptureGroupsWin]::ClipboardText($script:clipboardWindow.Handle,$script:sentinel) }
    # Let clipboard viewers finish processing this setup write before the app
    # receives its copy command; these external listeners briefly hold the lock.
    Start-Sleep -Milliseconds 200
}
function Verify-Copy([string]$Name,[string]$Expected,[scriptblock]$Action) {
    Set-Sentinel
    & $Action
    Wait-For { (Clipboard-Text) -eq $Expected } $Name 5000
    Check $true $Name @{copied=Clipboard-Text}
    Check ([CaptureGroupsWin]::IsWindow($script:pin) -and @([CaptureGroupsWin]::Windows($gui.Id,'ptools.capture.view')).Count -eq 2) ($Name + ' keeps the original pin and history alive')
}
function Client-Rect([IntPtr]$Window) {
    $r=[CaptureGroupsWin+RECT]::new()
    if(-not [CaptureGroupsWin]::GetClientRect($Window,[ref]$r)) { throw 'GetClientRect failed' }
    $r
}
function Snapshot([IntPtr]$Window,[string]$Name) {
    # Native popup teardown posts a repaint; let it finish before asking Windows
    # to render the target into our verification bitmap.
    Start-Sleep -Milliseconds 120
    $dpi=[CaptureGroupsWin]::SetThreadDpiAwarenessContext([IntPtr](-4)); $bitmap=$null; $graphics=$null
    try {
        $r=[CaptureGroupsWin+RECT]::new()
        if(-not [CaptureGroupsWin]::GetWindowRect($Window,[ref]$r)) { throw 'GetWindowRect failed' }
        $bitmap=[Drawing.Bitmap]::new($r.Right-$r.Left,$r.Bottom-$r.Top); $graphics=[Drawing.Graphics]::FromImage($bitmap)
        [void][CaptureGroupsWin]::RedrawWindow($Window,[IntPtr]::Zero,[IntPtr]::Zero,0x185)
        $dc=$graphics.GetHdc()
        try { if(-not [CaptureGroupsWin]::PrintWindow($Window,$dc,2)) { throw 'PrintWindow failed' } } finally { $graphics.ReleaseHdc($dc) }
        $bitmap.Save((Join-Path $runRoot $Name),[Drawing.Imaging.ImageFormat]::Png)
    } finally {
        if($graphics) { $graphics.Dispose() }; if($bitmap) { $bitmap.Dispose() }
        [void][CaptureGroupsWin]::SetThreadDpiAwarenessContext($dpi)
    }
}
function Mouse-Point([IntPtr]$Window,[uint32]$Message,[int]$X,[int]$Y,[long]$Flags=0) {
    [void][CaptureGroupsWin]::Send($Window,$Message,$Flags,($X -band 0xffff) -bor (($Y -band 0xffff) -shl 16))
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
    $scale=[Math]::Max(96,[int][CaptureGroupsWin]::GetDpiForWindow($script:toolbar))
    while($true) {
        $cell=[int][Math]::Floor((38*$scale+48)/96); $pad=[int][Math]::Floor((8*$scale+48)/96)
        $columns=[Math]::Min($toolbarItems.Count,[Math]::Max(1,[int][Math]::Floor(($r.Right-2*$pad)/$cell)))
        $style=[int][Math]::Floor((24*$scale+48)/96); $width=$columns*$cell+2*$pad
        $rows=[int][Math]::Ceiling($toolbarItems.Count/$columns); $styleColumns=[Math]::Max(1,[int][Math]::Floor(($width-2*$pad)/$style))
        $height=2*$pad+$rows*$cell+[int][Math]::Ceiling(12/$styleColumns)*$style+[int][Math]::Floor((28*$scale+48)/96)
        if($scale -le 96 -or ($width -le $r.Right -and $height -le $r.Bottom)) { break }; $scale--
    }
    $left=0; $top=0
    if($script:toolbarAnchor) {
        $left=[Math]::Clamp($script:toolbarAnchor.Right-$width,0,[Math]::Max(0,$r.Right-$width))
        if($script:toolbarAnchor.Bottom+$pad+$height -le $r.Bottom) { $top=$script:toolbarAnchor.Bottom+$pad }
        elseif($script:toolbarAnchor.Top-$pad -ge $height) { $top=$script:toolbarAnchor.Top-$pad-$height }
        else { $top=[Math]::Max(0,$r.Bottom-$height) }
    }
    @{cell=$cell;pad=$pad;columns=$columns;rows=$rows;style=$style;style_columns=$styleColumns;left=$left;top=$top;scale=$scale;width=$width;height=$height}
}
function Toolbar-Click([int]$Id) {
    $g=Toolbar-Geometry
    if($Id -eq 38) { $x=$g.pad+(8%$g.style_columns)*$g.style+[int]($g.style/2); $y=$g.pad+$g.rows*$g.cell+[int][Math]::Floor(8/$g.style_columns)*$g.style+[int]($g.style/2) }
    else {
        $i=[Array]::IndexOf($toolbarItems,$Id)
        if($i -lt 0) { throw "Unknown toolbar item $Id" }
        $x=$g.pad+($i%$g.columns)*$g.cell+[int]($g.cell/2); $y=$g.pad+[int][Math]::Floor($i/$g.columns)*$g.cell+[int]($g.cell/2)
    }
    Mouse-Point $script:toolbar 0x0201 ($g.left+$x) ($g.top+$y) 1
    Mouse-Point $script:toolbar 0x0202 ($g.left+$x) ($g.top+$y)
}
function Key([ushort]$Code,[bool]$Ctrl=$false) {
    foreach($modifier in @(0x10,0x11,0x12,0x5b,0x5c)) {
        if([CaptureGroupsWin]::GetAsyncKeyState($modifier) -lt 0) { throw 'A modifier key is held; keyboard checks stopped to avoid interfering with user input' }
    }
    if(-not [CaptureGroupsWin]::Focus($script:pin)) { throw 'Cannot focus our private pin for the keyboard check' }
    [CaptureGroupsWin]::Key($Code,$Ctrl)
    Start-Sleep -Milliseconds 150
}
function Shortcut([ushort]$Code) {
    [void][CaptureGroupsWin]::Send($script:pin,0x0100,$Code)
    [void][CaptureGroupsWin]::Send($script:pin,0x0101,$Code)
    Start-Sleep -Milliseconds 100
}
function Key-In([IntPtr]$Window,[ushort]$Code,[bool]$Ctrl=$false,[bool]$Shift=$false) {
    foreach($modifier in @(0x10,0x11,0x12,0x5b,0x5c)) {
        if([CaptureGroupsWin]::GetAsyncKeyState($modifier) -lt 0) { throw 'A modifier key is held; refusing keyboard input' }
    }
    if([CaptureGroupsWin]::GetForegroundWindow() -ne [CaptureGroupsWin]::GetAncestor($Window,2) -or [CaptureGroupsWin]::FocusedWindow($Window) -ne $Window) { throw 'Our private child is not focused; refusing keyboard input rather than committing it by activating the parent' }
    [CaptureGroupsWin]::Key($Code,$Ctrl,$Shift)
    Start-Sleep -Milliseconds 150
}
function Window-Rect([IntPtr]$Window) {
    $r=[CaptureGroupsWin+RECT]::new()
    if(-not [CaptureGroupsWin]::GetWindowRect($Window,[ref]$r)) { throw 'GetWindowRect failed' }; $r
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
    $script:clipboardTouched=$true; $before=[CaptureGroupsWin]::GetClipboardSequenceNumber()
    $payload=[CaptureGroupsWin]::CopyAndReadImage($script:pin)
    $after=[CaptureGroupsWin]::GetClipboardSequenceNumber()
    if($before -eq $after) { throw 'Image copy did not replace the clipboard' }
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
        $script:results.clipboard_changes_during_deselection+=@{test=$Name;actual=$actual;sentinel=$sentinel;owner=[CaptureGroupsWin]::Describe([CaptureGroupsWin]::GetClipboardOwner())}
    }
}
function Native-PinDrag([double]$X,[double]$Y,[int]$Dx,[int]$Dy) {
    foreach($key in @(0x01,0x02,0x10,0x11,0x12,0x5b,0x5c)) {
        if([CaptureGroupsWin]::GetAsyncKeyState($key) -lt 0) { throw 'Mouse button or modifier held; refusing native mouse input' }
    }
    if(-not [CaptureGroupsWin]::Focus($script:pin)) { throw 'Cannot focus our private pin for mouse input' }
    $p=Image-Point $X $Y; $r=Window-Rect $script:pin; $x=$r.Left+$p[0]; $y=$r.Top+$p[1]
    [CaptureGroupsWin]::Mouse($x,$y,0)
    [CaptureGroupsWin]::Mouse($x,$y,2)
    try {
        for($i=1;$i -le 8;$i++) { [CaptureGroupsWin]::Mouse(($x+[int]($Dx*$i/8)),($y+[int]($Dy*$i/8)),0); Start-Sleep -Milliseconds 25 }
    } finally { [CaptureGroupsWin]::Mouse(($x+$Dx),($y+$Dy),4) }
    Start-Sleep -Milliseconds 150
}
function Toolbar-Fill([string]$Name,[int]$Id) {
    $g=Toolbar-Geometry; $i=[Array]::IndexOf($toolbarItems,$Id)
    $bitmap=[Drawing.Bitmap]::new((Join-Path $runRoot $Name))
    try { $bitmap.GetPixel(($g.left+$g.pad+($i%$g.columns)*$g.cell+4),($g.top+$g.pad+[int][Math]::Floor($i/$g.columns)*$g.cell+4)).ToArgb() } finally { $bitmap.Dispose() }
}

$groups=@(
    @{anchor=1;ids=@(1,2);names=@('矩形','椭圆');default=1;keys=@(0x52,0x45)},
    @{anchor=3;ids=@(17,18,3);names=@('直线','折线','箭头');default=3;keys=@(0x4c,0x46,0x41)},
    @{anchor=4;ids=@(4,5);names=@('画笔','荧光笔');default=4;keys=@(0x50,0x48)}
)
$script:toolbarAnchor=$null
function Toolbar-Point([int]$Id,[bool]$Arrow=$false) {
    $g=Toolbar-Geometry; $i=[Array]::IndexOf($toolbarItems,$Id)
    if($i -lt 0) { throw "Unknown toolbar anchor $Id" }
    $x=$g.left+$g.pad+($i%$g.columns)*$g.cell
    $y=$g.top+$g.pad+[int][Math]::Floor($i/$g.columns)*$g.cell
    $offset=if($Arrow) { $g.cell-[int][Math]::Floor((5*$g.scale+48)/96) } else { [int][Math]::Floor((12*$g.scale+48)/96) }
    $x+=$offset; $y+=[int]($g.cell/2)
    @([int]$x,[int]$y)
}
function Group-Open([object]$Group,[string]$Name,[bool]$FocusOwner=$true) {
    if($FocusOwner -and -not [CaptureGroupsWin]::Focus($script:pin)) { throw 'Cannot focus our private menu owner' }
    $p=Toolbar-Point $Group.anchor $true
    # TrackPopupMenu owns a nested message loop. Never synchronously SendMessage
    # the toolbar click: its return depends on the later menu choice.
    if($FocusOwner) {
        [void][CaptureGroupsWin]::PostMessage($script:toolbar,0x0201,[IntPtr]1,[IntPtr](($p[0] -band 0xffff) -bor (($p[1] -band 0xffff) -shl 16)))
    } else {
        $toolbarRect=Window-Rect $script:toolbar
        [CaptureGroupsWin]::Mouse(($toolbarRect.Left+$p[0]),($toolbarRect.Top+$p[1]),2)
        [CaptureGroupsWin]::Mouse(($toolbarRect.Left+$p[0]),($toolbarRect.Top+$p[1]),4)
    }
    Wait-For { $script:menu=@([CaptureGroupsWin]::Windows($script:activePid,'#32768'))[0]; $null -ne $script:menu } ('native popup: '+$Name) 3000
    $script:menuHandle=[IntPtr][CaptureGroupsWin]::Send($script:menu,0x01e1)
    $script:menuRows=[CaptureGroupsWin]::MenuRows($script:menu,$script:menuHandle)
    $actual=@($script:menuRows | ForEach-Object { [int]$_.Id })
    Check (($actual -join ',') -eq ($Group.ids -join ',')) ($Name+' native popup exposes the exact group members') @{ids=$actual}
    Check ((@($script:menuRows | ForEach-Object { $_.Text }) -join ',') -eq ($Group.names -join ',')) ($Name+' native popup exposes Chinese labels') @{labels=@($script:menuRows | ForEach-Object { $_.Text })}
    Check (@($script:menuRows | Where-Object { ($_.Type -band 0x100) -ne 0 }).Count -eq $Group.ids.Count) ($Name+' native popup uses icon and description owner drawing')
    Start-Sleep -Milliseconds 100
    $r=Window-Rect $script:menu
    $image=[Drawing.Bitmap]::new($r.Right-$r.Left,$r.Bottom-$r.Top); $graphics=[Drawing.Graphics]::FromImage($image)
    try { $graphics.CopyFromScreen($r.Left,$r.Top,0,0,$image.Size); $image.Save((Join-Path $runRoot ($Name+'-menu.png')),[Drawing.Imaging.ImageFormat]::Png) }
    finally { $graphics.Dispose(); $image.Dispose() }
    $script:menuRows
}
function Menu-Key([ushort]$Code) {
    foreach($modifier in @(0x10,0x11,0x12,0x5b,0x5c)) { if([CaptureGroupsWin]::GetAsyncKeyState($modifier) -lt 0) { throw 'Modifier held; refusing menu keyboard input' } }
    if(-not [CaptureGroupsWin]::IsWindow($script:menu) -or [CaptureGroupsWin]::GetForegroundWindow() -ne [CaptureGroupsWin]::GetAncestor($script:pin,2)) { throw 'Our private menu owner is not foreground' }
    [CaptureGroupsWin]::Key($Code); Start-Sleep -Milliseconds 80
}
function Menu-Choose([int]$Index,[bool]$Mouse=$false) {
    if($Mouse) {
        if(-not [CaptureGroupsWin]::IsWindow($script:menu)) { throw 'Our private menu has closed before the mouse choice' }
        $r=$script:menuRows[$Index].Bounds; $x=[int](($r.Left+$r.Right)/2); $y=[int](($r.Top+$r.Bottom)/2)
        [CaptureGroupsWin]::Mouse($x,$y,2); [CaptureGroupsWin]::Mouse($x,$y,4); Start-Sleep -Milliseconds 150
    } else {
        Menu-Key 0x24
        for($i=0;$i -lt $Index;$i++) { Menu-Key 0x28 }
        Menu-Key 0x0d
    }
    Wait-For { @([CaptureGroupsWin]::Windows($script:activePid,'#32768')).Count -eq 0 } 'menu selection closes only the popup' 3000
}
function Menu-Cancel {
    Menu-Key 0x1b
    Wait-For { @([CaptureGroupsWin]::Windows($script:activePid,'#32768')).Count -eq 0 } 'Escape dismisses the popup' 3000
}
function Group-Checked([object[]]$Rows,[int]$Id,[string]$Name) {
    $checked=@($Rows | Where-Object { ($_.State -band 8) -ne 0 } | ForEach-Object { [int]$_.Id })
    Check ($checked.Count -eq 1 -and $checked[0] -eq $Id) $Name @{checked_ids=$checked}
}
function Group-Main([object]$Group,[string]$Name) {
    $p=Toolbar-Point $Group.anchor
    Mouse-Point $script:toolbar 0x0201 $p[0] $p[1] 1; Mouse-Point $script:toolbar 0x0202 $p[0] $p[1]
    Check (@([CaptureGroupsWin]::Windows($script:activePid,'#32768')).Count -eq 0) ($Name+' main icon activates directly without opening a popup')
}
function Tool-Ink([string]$ImageName,[int]$Id) {
    $g=Toolbar-Geometry; $i=[Array]::IndexOf($toolbarItems,$Id)
    $left=$g.left+$g.pad+($i%$g.columns)*$g.cell; $top=$g.top+$g.pad+[int][Math]::Floor($i/$g.columns)*$g.cell
    $bitmap=[Drawing.Bitmap]::new((Join-Path $runRoot $ImageName))
    try {
        $values=[Collections.Generic.List[int]]::new()
        for($y=3;$y -lt $g.cell-3;$y++) { for($x=3;$x -lt [int]($g.cell*0.65);$x++) { $pixel=$bitmap.GetPixel($left+$x,$top+$y); $values.Add($(if([Math]::Max($pixel.R,[Math]::Max($pixel.G,$pixel.B)) -gt 160) { 1 } else { 0 })) } }
        $values -join ''
    } finally { $bitmap.Dispose() }
}
function Draw-Member([int]$Id,[IntPtr]$Window,[double[]]$Origin,[double]$ScaleX=1,[double]$ScaleY=1) {
    $point={ param($x,$y) @([int][Math]::Round($Origin[0]+$x*$ScaleX),[int][Math]::Round($Origin[1]+$y*$ScaleY)) }
    if($Id -eq 18) {
        foreach($xy in @(@(130,260),@(260,335),@(400,260))) {
            $p=& $point $xy[0] $xy[1]; Mouse-Point $Window 0x0200 $p[0] $p[1]; Mouse-Point $Window 0x0201 $p[0] $p[1] 1; Mouse-Point $Window 0x0202 $p[0] $p[1]
        }
        [void][CaptureGroupsWin]::Send($Window,0x0100,0x0d)
    } else {
        $a=& $point 130 260; $b=& $point 400 335
        Mouse-Point $Window 0x0201 $a[0] $a[1] 1
        if($Id -in @(4,5)) { foreach($xy in @(@(220,285),@(270,320),@(340,295))) { $p=& $point $xy[0] $xy[1]; Mouse-Point $Window 0x0200 $p[0] $p[1] 1 } }
        Mouse-Point $Window 0x0200 $b[0] $b[1] 1; Mouse-Point $Window 0x0202 $b[0] $b[1]
    }
}

try {
    if([Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') { throw 'Run with pwsh -STA -File scripts/verify-capture-groups.ps1; no clipboard was changed' }
    $originalForeground=[CaptureGroupsWin]::GetForegroundWindow()
    $originalCursor=[CaptureGroupsWin+POINT]::new(); [void][CaptureGroupsWin]::GetCursorPos([ref]$originalCursor)
    $originalDpi=[CaptureGroupsWin]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $sequence=[CaptureGroupsWin]::GetClipboardSequenceNumber(); $source=[Windows.Forms.Clipboard]::GetDataObject()
    $clipboardSnapshot=[Windows.Forms.DataObject]::new(); $clipboardWasEmpty=$null -eq $source; $formats=@()
    if($source) {
        $formats=@($source.GetFormats($false)); $clipboardWasEmpty=$formats.Count -eq 0
        foreach($format in $formats) {
            $value=$source.GetData($format,$false)
            if($value -is [Drawing.Image]) { $value=$value.Clone(); $clipboardCopies.Add($value) }
            elseif($value -is [IO.Stream]) {
                if(-not $value.CanSeek) { throw "Cannot snapshot nonseekable clipboard stream: $format; no clipboard was changed" }
                $stream=[IO.MemoryStream]::new(); $position=$value.Position
                try { $value.Position=0; $value.CopyTo($stream) } finally { $value.Position=$position }
                $stream.Position=0; $value=$stream; $clipboardCopies.Add($value)
            } elseif($value -is [Array]) { $value=$value.Clone() }
            elseif($null -ne $value -and $value -isnot [string]) { throw "Unsupported clipboard format: $format; no clipboard was changed" }
            if($null -eq $value) { throw "Cannot materialize clipboard format: $format; no clipboard was changed" }
            $clipboardSnapshot.SetData($format,$false,$value)
        }
    }
    if([CaptureGroupsWin]::GetClipboardSequenceNumber() -ne $sequence) { throw 'Clipboard changed during snapshot; no clipboard was changed' }
    $clipboardReady=$true; $results.clipboard_formats=$formats
    $clipboardWindow=[Windows.Forms.NativeWindow]::new()
    $params=[Windows.Forms.CreateParams]::new(); $params.Caption='ptools private grouped toolbar verification clipboard'; $params.Style=[int]0x80000000
    $clipboardWindow.CreateHandle($params)
    $bitmap=[Drawing.Bitmap]::new(960,420); $bitmap.SetResolution(96,96)
    $graphics=[Drawing.Graphics]::FromImage($bitmap); $font=[Drawing.Font]::new('Microsoft YaHei',32,[Drawing.FontStyle]::Regular,[Drawing.GraphicsUnit]::Pixel)
    try {
        $graphics.Clear([Drawing.Color]::White); $graphics.TextRenderingHint=[Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        $graphics.DrawString('中文选择测试 Hello ptools 12345',$font,[Drawing.Brushes]::Black,32,36)
        $graphics.DrawString('第二行 Second line 67890',$font,[Drawing.Brushes]::Black,32,116)
        foreach($path in @((Join-Path $images '1-1.png'),(Join-Path $images '1-1-thumb.png'),(Join-Path $runRoot 'fixture.png'))) { $bitmap.Save($path,[Drawing.Imaging.ImageFormat]::Png) }
    } finally { $font.Dispose(); $graphics.Dispose(); $bitmap.Dispose() }
    @(@{id='1-1';created=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds();source='私有分组工具栏验收';width=960;height=420}) | ConvertTo-Json -AsArray | Set-Content -LiteralPath (Join-Path $images 'index.json') -Encoding utf8NoBOM
    $info=[Diagnostics.ProcessStartInfo]::new($Executable); $info.UseShellExecute=$false; $info.CreateNoWindow=$true; $info.WindowStyle=[Diagnostics.ProcessWindowStyle]::Hidden; $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true
    $info.ArgumentList.Add('--ocr'); $info.ArgumentList.Add((Join-Path $images '1-1.png'))
    $probe=[Diagnostics.Process]::Start($info)
    try {
        $read=$probe.StandardOutput.ReadToEndAsync(); $errors=$probe.StandardError.ReadToEndAsync()
        if(-not $probe.WaitForExit(30000)) { $probe.Kill(); throw 'Private fixture OCR timed out' }
        if($probe.ExitCode -ne 0) { throw "Fixture OCR failed: $($errors.GetAwaiter().GetResult())" }; $recognition=$read.GetAwaiter().GetResult() | ConvertFrom-Json
    } finally { $probe.Dispose() }
    $word=@($recognition.words | Where-Object { $_.text -eq 'Hello' })[0]
    Check ($null -ne $word) 'Private fixture has a recognized Hello word'; $box=$word.bounds; $wordY=$box[1]+$box[3]/2
    Set-Sentinel
    $gui=Start-Process -FilePath $Executable -ArgumentList @('--action','history','--data-dir',('"'+$dataRoot+'"')) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $runRoot 'pin.stdout.log') -RedirectStandardError (Join-Path $runRoot 'pin.stderr.log') -PassThru
    $script:activePid=$gui.Id; $results.process_id=$gui.Id
    Wait-For { $script:history=@([CaptureGroupsWin]::Windows($gui.Id,'ptools.capture.view'))[0]; $null -ne $history -and [CaptureGroupsWin]::GetDlgItem($history,1) -ne [IntPtr]::Zero } 'private history'
    [void][CaptureGroupsWin]::Send($history,0x0111,(1 -bor (2 -shl 16)))
    Wait-For { $script:pin=@([CaptureGroupsWin]::Windows($gui.Id,'ptools.capture.view') | Where-Object { $_ -ne $history })[0]; $null -ne $pin } 'private pin'
    Wait-Ocr $dataRoot; Key 0x20
    Wait-For { $script:toolbar=@([CaptureGroupsWin]::Windows($gui.Id,'ptools.capture.toolbar'))[0]; $null -ne $toolbar } 'floating pin toolbar'
    Snapshot $toolbar 'pin-toolbar-initial.png'
    Check ($toolbarItems.Count -eq 22) 'Main toolbar contains 22 actions after merging 7 drawing tools into 3 groups'
    $g=Toolbar-Geometry
    Check ($g.width -le (Client-Rect $toolbar).Right -and $g.height -le (Client-Rect $toolbar).Bottom) 'Floating toolbar geometry fits every main and style action'
    Check ((Toolbar-Fill 'pin-toolbar-initial.png' 12) -eq (Toolbar-Fill 'pin-toolbar-initial.png' 1)) 'OCR switch remains independently highlighted with the initial shape tool'
    if($originalForeground -ne [IntPtr]::Zero -and [CaptureGroupsWin]::IsWindow($originalForeground)) {
        if(-not [CaptureGroupsWin]::Focus($originalForeground)) { throw 'Cannot restore original app foreground for the nonactivating toolbar check' }
        Check ([CaptureGroupsWin]::GetForegroundWindow() -eq [CaptureGroupsWin]::GetAncestor($originalForeground,2)) 'The original external app is foreground before the floating toolbar dropdown opens'
        $rows=@(Group-Open $groups[0] 'pin-external-foreground' $false); Menu-Choose 1
        Check ([CaptureGroupsWin]::IsWindow($pin)) 'Floating toolbar dropdown accepts real keyboard input after another app was foreground'
        Shortcut 0x52
    }
    foreach($group in $groups) {
        $rows=@(Group-Open $group ('pin-default-'+$group.anchor)); Group-Checked $rows $group.default ('Group '+$group.anchor+' starts with its default choice'); Menu-Cancel
        Check ([CaptureGroupsWin]::IsWindow($pin) -and [CaptureGroupsWin]::IsWindow($toolbar)) ('Escape from group '+$group.anchor+' keeps pin and toolbar alive')
        $signatures=@{}
        for($index=0;$index -lt $group.ids.Count;$index++) {
            $id=$group.ids[$index]; $name='pin-'+$id
            $rows=@(Group-Open $group ($name+'-choose')); Menu-Choose $index ($index -eq 0)
            Group-Main $group $name; Snapshot $toolbar ($name+'-toolbar.png')
            $signatures[$id]=Tool-Ink ($name+'-toolbar.png') $group.anchor
            Check ((Toolbar-Fill ($name+'-toolbar.png') 12) -eq (Toolbar-Fill ($name+'-toolbar.png') $group.anchor)) ($name+' grouped tool and OCR switch are highlighted together')
            Copy-PinImage ($name+'-before.png')
            $r=Client-Rect $pin; Draw-Member $id $pin @(2,2) (($r.Right-4)/960) (($r.Bottom-4)/420)
            Copy-PinImage ($name+'-drawn.png')
            $changed=Changed-Pixels ($name+'-before.png') ($name+'-drawn.png') @(100,235,430,360)
            Check ($changed -gt 40) ($name+' selected member draws real exported pixels') @{changed_pixels=$changed}
            Toolbar-Click 21; Copy-PinImage ($name+'-undone.png')
            Check ((Changed-Pixels ($name+'-before.png') ($name+'-undone.png') @(0,0,960,420)) -eq 0) ($name+' undo removes the grouped annotation as one action')
            $rows=@(Group-Open $group ($name+'-remembered')); Group-Checked $rows $id ($name+' dropdown remembers the selected member'); Menu-Cancel
            Shortcut 0x56; Group-Main $group ($name+'-reactivate')
            Snapshot $toolbar ($name+'-reactivated-toolbar.png')
            Check ((Tool-Ink ($name+'-toolbar.png') $group.anchor) -eq (Tool-Ink ($name+'-reactivated-toolbar.png') $group.anchor)) ($name+' main button retains the selected icon after switching tools')
            Wait-Ocr $dataRoot
            Verify-Copy ($name+' still selects recognized image text') 'Hello' { Drag-Image ($box[0]+2) $wordY ($box[0]+$box[2]-2) $wordY }
        }
        Check (@($signatures.Values | Select-Object -Unique).Count -eq $group.ids.Count) ('Group '+$group.anchor+' displays a different main icon for every member')
        for($index=0;$index -lt $group.ids.Count;$index++) {
            $id=$group.ids[$index]; Shortcut ([ushort]$group.keys[$index]); Snapshot $toolbar ('pin-shortcut-'+$id+'.png')
            Check ((Tool-Ink ('pin-shortcut-'+$id+'.png') $group.anchor) -eq $signatures[$id]) ('Shortcut selects member '+$id+' and updates its displayed icon')
            $rows=@(Group-Open $group ('pin-shortcut-'+$id)); Group-Checked $rows $id ('Shortcut updates remembered group choice '+$id); Menu-Cancel
        }
    }
    Shortcut 0x45; Key 0x20
    Check (-not [CaptureGroupsWin]::IsWindowVisible($toolbar)) 'Space hides the floating toolbar after choosing ellipse'
    Key 0x20
    Check ([CaptureGroupsWin]::IsWindowVisible($toolbar)) 'Space reopens the floating toolbar'
    $rows=@(Group-Open $groups[0] 'pin-reopen-shape'); Group-Checked $rows 2 'Reopening pin editing remembers the ellipse choice'; Menu-Cancel
    Copy-PinImage 'pin-before-outside-cancel.png'
    $rows=@(Group-Open $groups[0] 'pin-outside-cancel'); $r=Window-Rect $pin
    [CaptureGroupsWin]::Mouse(($r.Left+700),($r.Top+220),2); [CaptureGroupsWin]::Mouse(($r.Left+700),($r.Top+220),4)
    Wait-For { @([CaptureGroupsWin]::Windows($gui.Id,'#32768')).Count -eq 0 } 'outside click closes group popup' 3000
    Check ([CaptureGroupsWin]::IsWindow($pin)) 'Outside click cancels the popup while preserving the pin'
    Copy-PinImage 'pin-after-outside-cancel.png'
    Check ((Changed-Pixels 'pin-before-outside-cancel.png' 'pin-after-outside-cancel.png' @(0,0,960,420)) -eq 0) 'Outside menu click is consumed without drawing on the underlying pin'
    $rows=@(Group-Open $groups[0] 'pin-after-outside-cancel'); Group-Checked $rows 2 'Outside cancellation preserves the remembered ellipse choice'; Menu-Cancel
    Toolbar-Click 38; Snapshot $toolbar 'pin-mosaic-toolbar.png'
    Check ((Toolbar-Fill 'pin-mosaic-toolbar.png' 12) -eq (Toolbar-Fill 'pin-mosaic-toolbar.png' 4)) 'Mosaic stays in the style palette while its brush group and the OCR switch remain highlighted'

    # Capture only the generated private fixture; dropdown interaction in a real
    # screenshot must use the same split buttons as the floating pin toolbar.
    [void][CaptureGroupsWin]::Send($pin,0x0111,0); Set-Sentinel; Native-PinDrag 700 250 0 0
    [void][CaptureGroupsWin]::Send($pin,0x0111,25)
    $pinRect=Window-Rect $pin; $workArea=[Windows.Forms.Screen]::FromHandle($pin).WorkingArea
    [void][CaptureGroupsWin]::SetWindowPos($pin,[IntPtr](-1),$workArea.Left+24,$workArea.Top+24,$pinRect.Right-$pinRect.Left,$pinRect.Bottom-$pinRect.Top,0x10)
    [void][CaptureGroupsWin]::RedrawWindow($pin,[IntPtr]::Zero,[IntPtr]::Zero,0x185); Start-Sleep -Milliseconds 200
    $pinRect=Window-Rect $pin; $pinClient=Client-Rect $pin
    $overlayData=Join-Path $runRoot 'overlay-data'; New-Item -ItemType Directory -Path $overlayData -Force | Out-Null
    @{include_cursor=$false} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $overlayData 'settings.json') -Encoding utf8NoBOM
    $overlayGui=Start-Process -FilePath $Executable -ArgumentList @('--action','capture','--data-dir',('"'+$overlayData+'"')) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $runRoot 'capture.stdout.log') -RedirectStandardError (Join-Path $runRoot 'capture.stderr.log') -PassThru
    Wait-For { $script:overlay=@([CaptureGroupsWin]::Windows($overlayGui.Id,'ptools.capture.view'))[0]; $null -ne $overlay } 'private screenshot overlay'
    $overlayRect=Window-Rect $overlay; $cropX=$pinRect.Left+2-$overlayRect.Left; $cropY=$pinRect.Top+2-$overlayRect.Top; $cropWidth=$pinClient.Right-4; $cropHeight=$pinClient.Bottom-4
    Mouse-Point $overlay 0x0201 $cropX $cropY 1; Mouse-Point $overlay 0x0200 ($cropX+$cropWidth) ($cropY+$cropHeight) 1; Mouse-Point $overlay 0x0202 ($cropX+$cropWidth) ($cropY+$cropHeight)
    Wait-Ocr $overlayData; $originalPin=$pin; $script:pin=$overlay; $script:toolbar=$overlay; $script:activePid=$overlayGui.Id
    $script:toolbarAnchor=[CaptureGroupsWin+RECT]::new(); $toolbarAnchor.Left=$cropX; $toolbarAnchor.Top=$cropY; $toolbarAnchor.Right=$cropX+$cropWidth; $toolbarAnchor.Bottom=$cropY+$cropHeight
    Snapshot $overlay 'capture-toolbar-initial.png'
    foreach($group in $groups) {
        for($index=0;$index -lt $group.ids.Count;$index++) {
            $id=$group.ids[$index]; $name='capture-'+$id
            $rows=@(Group-Open $group ($name+'-choose')); Menu-Choose $index ($index -eq 0); Group-Main $group $name
            Draw-Member $id $overlay @($cropX,$cropY) ($cropWidth/960) ($cropHeight/420)
            Snapshot $overlay ($name+'-drawn.png')
            [void][CaptureGroupsWin]::Send($overlay,0x0111,21)
            Snapshot $overlay ($name+'-undone.png')
            $region=@([int]($cropX+100*$cropWidth/960),[int]($cropY+235*$cropHeight/420),[int]($cropX+430*$cropWidth/960),[int]($cropY+360*$cropHeight/420))
            Check ((Changed-Pixels ($name+'-undone.png') ($name+'-drawn.png') $region) -gt 30) ($name+' selected menu member draws in the actual screenshot')
            Check ([CaptureGroupsWin]::IsWindow($overlay)) ($name+' dropdown interaction keeps the screenshot open')
        }
    }
    $rows=@(Group-Open $groups[0] 'capture-escape'); Menu-Cancel
    Check ([CaptureGroupsWin]::IsWindow($overlay)) 'Escape from screenshot dropdown preserves the screenshot window'
    [void][CaptureGroupsWin]::Send($overlay,0x0111,1); Set-Sentinel; Wait-Ocr $overlayData
    $screenLeft=[int][Math]::Round($cropX+($box[0]+2)*$cropWidth/960); $screenRight=[int][Math]::Round($cropX+($box[0]+$box[2]-2)*$cropWidth/960); $screenY=[int][Math]::Round($cropY+$wordY*$cropHeight/420)
    Mouse-Point $overlay 0x0201 $screenLeft $screenY 1; Mouse-Point $overlay 0x0200 $screenRight $screenY 1; Mouse-Point $overlay 0x0202 $screenRight $screenY
    Wait-For { (Clipboard-Text) -eq 'Hello' } 'Screenshot OCR remains selectable with a grouped shape tool' 5000
    Check $true 'Screenshot OCR stays independent of grouped drawing tools'
    [void][CaptureGroupsWin]::Send($overlay,0x0111,9)
    Wait-For { [CaptureGroupsWin]::IsClipboardFormatAvailable(17) } 'complete screenshot copies a real image' 5000
    $captured=Clipboard-Image
    try { Check ($captured.Width -eq $cropWidth -and $captured.Height -eq $cropHeight) 'Screenshot export preserves region dimensions after grouped tool use'; $captured.Save((Join-Path $runRoot 'capture-export.png'),[Drawing.Imaging.ImageFormat]::Png) } finally { $captured.Dispose() }
    Check ($overlayGui.WaitForExit(5000)) 'Completing the private screenshot exits its process'; $overlayGui.Dispose(); $overlayGui=$null
    [void][CaptureGroupsWin]::PostMessage($originalPin,0x0010,[IntPtr]::Zero,[IntPtr]::Zero); [void][CaptureGroupsWin]::PostMessage($history,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    Check ($gui.WaitForExit(5000)) 'Closing private pin and history exits the verification process'; $results.status='passed'
} catch {
    $results.status='failed'; $results.error=$_.Exception.Message
    $results.clipboard_owner_at_failure=[CaptureGroupsWin]::Describe([CaptureGroupsWin]::GetClipboardOwner())
    $results.clipboard_text_at_failure=Clipboard-Text
    if($gui) { $gui.Refresh(); $results.pin_process_exited_at_failure=$gui.HasExited; $results.pin_windows_at_failure=@([CaptureGroupsWin]::Windows($gui.Id,'',$false) | ForEach-Object { [CaptureGroupsWin]::Describe($_) }) }
    throw
} finally {
    foreach($process in @($overlayGui,$gui)) { if($process) { $process.Refresh(); if(-not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }; $process.Dispose() } }
    if($clipboardReady -and $clipboardTouched) {
        try { Invoke-ClipboardWrite { if($clipboardWasEmpty) { [Windows.Forms.Clipboard]::Clear() } else { [Windows.Forms.Clipboard]::SetDataObject($clipboardSnapshot,$true) } }; $results.clipboard_restored=$true }
        catch { $results.clipboard_restored=$false; $results.clipboard_restore_error=$_.Exception.Message; $results.status='failed' }
    }
    foreach($copy in $clipboardCopies) { $copy.Dispose() }; if($clipboardWindow) { $clipboardWindow.DestroyHandle() }
    if($originalCursor) { [void][CaptureGroupsWin]::SetCursorPos($originalCursor.X,$originalCursor.Y) }
    if($originalForeground -ne [IntPtr]::Zero -and [CaptureGroupsWin]::IsWindow($originalForeground)) { [void][CaptureGroupsWin]::SetForegroundWindow($originalForeground) }
    if($originalDpi -ne [IntPtr]::Zero) { [void][CaptureGroupsWin]::SetThreadDpiAwarenessContext($originalDpi) }
    $results | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $runRoot 'results.json') -Encoding utf8NoBOM
    Write-Output "Evidence: $runRoot"
    if($results.clipboard_restored -eq $false) { throw 'Could not restore the original clipboard; see results.json' }
}
