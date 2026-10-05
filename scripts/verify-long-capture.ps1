param([string]$Executable='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $Executable) { $Executable=Join-Path $projectRoot 'dist/ptools/plugins/capture/ptools-capture.exe' }
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$runRoot=Join-Path $projectRoot ('artifacts/long-capture-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Path $runRoot -Force | Out-Null
$results=[ordered]@{timestamp=(Get-Date).ToString('o');executable=$Executable;status='running';checks=@();clipboard_restored=$null;input='Native mouse messages to our capture windows; private scrolling texture fixture';placement_note='Single-monitor UI exercises right outside, left outside and right inside. Left inside is reachable for a selection crossing a monitor edge and is covered by layout unit tests.'}
$gui=$null; $fixture=$null; $clipboardReady=$false; $clipboardTouched=$false
$originalForeground=[IntPtr]::Zero; $originalDpi=[IntPtr]::Zero
$clipboardCopies=[Collections.Generic.List[IDisposable]]::new()
function Check([bool]$Ok,[string]$Name,[object]$Evidence=$null) {
    $script:results.checks += [ordered]@{name=$Name;passed=$Ok;evidence=$Evidence}
    if(-not $Ok) { throw "FAILED: $Name" }; Write-Output "PASS: $Name"
}
function Wait-For([scriptblock]$Condition,[string]$Name,[int]$TimeoutMs=10000) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while($watch.ElapsedMilliseconds -lt $TimeoutMs) { if(& $Condition) { return }; Start-Sleep -Milliseconds 40 }
    throw "Timeout after $TimeoutMs ms: $Name"
}
Add-Type -AssemblyName System.Drawing,System.Windows.Forms
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Threading;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class CaptureLongWin {
  public delegate bool EnumProc(IntPtr h,IntPtr l);
  public delegate IntPtr WindowProc(IntPtr h,uint m,IntPtr w,IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct WNDCLASS {
    public uint Style; public WindowProc Proc; public int ClassExtra,WindowExtra; public IntPtr Instance,Icon,Cursor,Background;
    public string MenuName,ClassName;
  }
  [StructLayout(LayoutKind.Sequential)] public struct MSG { public IntPtr H; public uint M; public UIntPtr W; public IntPtr L; public uint Time; public int X,Y; public uint Private; }
  [StructLayout(LayoutKind.Sequential)] public struct PAINTSTRUCT { public IntPtr Dc; public int Erase; public RECT Rect; public int Restore,IncUpdate; [MarshalAs(UnmanagedType.ByValArray,SizeConst=32)] public byte[] Reserved; }
  [StructLayout(LayoutKind.Sequential)] public struct BITMAPINFO { public uint Size; public int Width,Height; public ushort Planes,Bits; public uint Compression,SizeImage; public int Xppm,Yppm; public uint Used,Important; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint Type; public UNION Data; }
  [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public KEY Key; [FieldOffset(0)] public MOUSE Mouse; }
  [StructLayout(LayoutKind.Sequential)] public struct KEY { public ushort Vk,Scan; public uint Flags,Time; public UIntPtr Extra; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSE { public int X,Y; public uint Data,Flags,Time; public UIntPtr Extra; }
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h,EnumProc callback,IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder name,int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int width,int height,uint flags);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll",EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr h,int index);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW")] private static extern IntPtr SendRaw(IntPtr h,uint msg,IntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
  [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
  [DllImport("user32.dll")] private static extern uint SendInput(uint count,INPUT[] input,int size);
  [DllImport("user32.dll")] private static extern bool RegisterHotKey(IntPtr h,int id,uint modifiers,uint key);
  [DllImport("user32.dll")] private static extern bool UnregisterHotKey(IntPtr h,int id);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr GetModuleHandle(string name);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern ushort RegisterClass(ref WNDCLASS c);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern bool UnregisterClass(string c,IntPtr instance);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr CreateWindowEx(uint ex,string c,string title,uint style,int x,int y,int width,int height,IntPtr parent,IntPtr menu,IntPtr instance,IntPtr param);
  [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetMessage(out MSG m,IntPtr h,uint min,uint max);
  [DllImport("user32.dll")] public static extern bool TranslateMessage(ref MSG m);
  [DllImport("user32.dll")] public static extern IntPtr DispatchMessage(ref MSG m);
  [DllImport("user32.dll")] public static extern IntPtr DefWindowProc(IntPtr h,uint m,IntPtr w,IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr BeginPaint(IntPtr h,out PAINTSTRUCT p);
  [DllImport("user32.dll")] public static extern bool EndPaint(IntPtr h,ref PAINTSTRUCT p);
  [DllImport("user32.dll")] public static extern bool InvalidateRect(IntPtr h,IntPtr r,bool erase);
  [DllImport("user32.dll")] public static extern bool UpdateWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern void PostQuitMessage(int code);
  [DllImport("gdi32.dll")] public static extern int StretchDIBits(IntPtr dc,int dx,int dy,int dw,int dh,int sx,int sy,int sw,int sh,IntPtr pixels,ref BITMAPINFO info,uint colors,uint rop);
  public static IntPtr[] Windows(uint pid,string match) {
    var result=new List<IntPtr>();
    EnumWindows((h,l)=>{GetWindowThreadProcessId(h,out uint found);var name=new StringBuilder(256);GetClassName(h,name,256);
      if(found==pid && (match=="" || name.ToString()==match) && IsWindowVisible(h)) result.Add(h);return true;},IntPtr.Zero);
    return result.ToArray();
  }
  public static string[] Children(IntPtr parent) {
    var result=new List<string>(); EnumChildWindows(parent,(h,l)=>{var name=new StringBuilder(256);GetClassName(h,name,256);result.Add(name.ToString());return true;},IntPtr.Zero);return result.ToArray();
  }
  public static long Send(IntPtr h,uint msg,long w=0,long l=0) {
    if(SendRaw(h,msg,new IntPtr(w),new IntPtr(l),2,2000,out UIntPtr result)==IntPtr.Zero) throw new Exception("Window message failed or timed out: "+msg);
    return unchecked((long)result.ToUInt64());
  }
  public static bool HotkeyFree(uint modifiers,uint key) {
    if(!RegisterHotKey(IntPtr.Zero,0x61f1,modifiers,key)) return false;
    UnregisterHotKey(IntPtr.Zero,0x61f1);return true;
  }
  public static void Key(ushort vk) {
    var input=new[]{new INPUT{Type=1,Data=new UNION{Key=new KEY{Vk=vk}}},new INPUT{Type=1,Data=new UNION{Key=new KEY{Vk=vk,Flags=2}}}};
    if(SendInput(2,input,Marshal.SizeOf<INPUT>())!=2) {SendInput(1,new[]{input[1]},Marshal.SizeOf<INPUT>());throw new Exception("SendInput key failed");}
  }
}
// This window belongs only to this script. It never scrolls or sends input to an
// unrelated app. A unique pixel texture gives every stitched row an exact oracle.
public sealed class CaptureScrollFixture : IDisposable {
  private readonly Thread thread; private readonly ManualResetEvent ready=new ManualResetEvent(false);
  private readonly int x,y,width,height; private const int contentHeight=1800;
  private readonly CaptureLongWin.WindowProc proc; private Exception failure;
  private uint[] pixels; private GCHandle pixelHandle; private int offset;
  public IntPtr Hwnd {get;private set;}
  public CaptureScrollFixture(int x,int y,int width,int height) {
    this.x=x;this.y=y;this.width=width;this.height=height;proc=OnMessage;
    thread=new Thread(Run);thread.SetApartmentState(ApartmentState.STA);thread.IsBackground=true;thread.Start();
    if(!ready.WaitOne(10000)) throw new Exception("Fixture thread did not start");if(failure!=null) throw failure;
  }
  public static uint Color(int x,int y) {
    uint v=unchecked((uint)x*1664525u ^ (uint)y*1013904223u);v^=v>>13;v=unchecked(v*2246822519u);v^=v>>16;
    return 0xff000000u | ((48u+(v&127))<<16) | ((55u+((v>>8)&127))<<8) | (66u+((v>>16)&127));
  }
  private void Run() {
    IntPtr dpi=CaptureLongWin.SetThreadDpiAwarenessContext(new IntPtr(-4));string name="ptools.long.fixture."+Guid.NewGuid().ToString("N");IntPtr instance=CaptureLongWin.GetModuleHandle(null);
    try {
      pixels=new uint[width*contentHeight];for(int row=0;row<contentHeight;row++) for(int col=0;col<width;col++) pixels[row*width+col]=Color(col,row);
      pixelHandle=GCHandle.Alloc(pixels,GCHandleType.Pinned);
      var c=new CaptureLongWin.WNDCLASS{Proc=proc,Instance=instance,ClassName=name};
      if(CaptureLongWin.RegisterClass(ref c)==0) throw new Exception("Fixture class registration failed");
      Hwnd=CaptureLongWin.CreateWindowEx(8,name,"ptools private scrolling fixture",0x90000000u,x,y,width,height,IntPtr.Zero,IntPtr.Zero,instance,IntPtr.Zero);
      if(Hwnd==IntPtr.Zero) throw new Exception("Fixture creation failed");CaptureLongWin.UpdateWindow(Hwnd);ready.Set();
      while(CaptureLongWin.GetMessage(out var message,IntPtr.Zero,0,0)>0) {CaptureLongWin.TranslateMessage(ref message);CaptureLongWin.DispatchMessage(ref message);}
    } catch(Exception e) {failure=e;ready.Set();}
    finally {if(pixelHandle.IsAllocated) pixelHandle.Free();CaptureLongWin.UnregisterClass(name,instance);CaptureLongWin.SetThreadDpiAwarenessContext(dpi);}
  }
  private IntPtr OnMessage(IntPtr h,uint m,IntPtr w,IntPtr l) {
    if(m==0x8001) {offset=w.ToInt32();CaptureLongWin.InvalidateRect(h,IntPtr.Zero,false);CaptureLongWin.UpdateWindow(h);return IntPtr.Zero;}
    if(m==0x000f) {
      IntPtr dc=CaptureLongWin.BeginPaint(h,out var paint);
      var info=new CaptureLongWin.BITMAPINFO{Size=40,Width=width,Height=-height,Planes=1,Bits=32};
      CaptureLongWin.StretchDIBits(dc,0,0,width,height,0,0,width,height,IntPtr.Add(pixelHandle.AddrOfPinnedObject(),offset*width*4),ref info,0,0x00cc0020);
      CaptureLongWin.EndPaint(h,ref paint);return IntPtr.Zero;
    }
    if(m==0x0010) {CaptureLongWin.DestroyWindow(h);return IntPtr.Zero;}
    if(m==0x0002) {CaptureLongWin.PostQuitMessage(0);return IntPtr.Zero;}
    return CaptureLongWin.DefWindowProc(h,m,w,l);
  }
  public void SetOffset(int value) {if(value<0 || value+height>contentHeight) throw new ArgumentOutOfRangeException("value");CaptureLongWin.Send(Hwnd,0x8001,value);}
  public static long Mismatches(IntPtr actual,int stride,int width,int height,int cropX,int cropY) {
    long different=0;var row=new int[width];
    for(int y=0;y<height;y++) {Marshal.Copy(IntPtr.Add(actual,y*stride),row,0,width);for(int x=0;x<width;x++) if((unchecked((uint)row[x])&0xffffffu)!=(Color(x+cropX,y+cropY)&0xffffffu)) different++;}
    return different;
  }
  public void Dispose() {if(Hwnd!=IntPtr.Zero && CaptureLongWin.IsWindow(Hwnd)) CaptureLongWin.PostMessage(Hwnd,0x0010,IntPtr.Zero,IntPtr.Zero);if(!thread.Join(5000)) throw new Exception("Fixture thread did not close");ready.Dispose();}
}
'@
function Invoke-ClipboardWrite([scriptblock]$Action) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while($true) { try { & $Action; return } catch { if($_.Exception.GetBaseException() -isnot [Runtime.InteropServices.ExternalException] -or $watch.ElapsedMilliseconds -ge 3000) { throw } }; Start-Sleep -Milliseconds 40 }
}
function Window-Rect([IntPtr]$Window) { $r=[CaptureLongWin+RECT]::new(); if(-not [CaptureLongWin]::GetWindowRect($Window,[ref]$r)) { throw 'GetWindowRect failed' }; $r }
function Rect-Array($r) { @($r.Left,$r.Top,$r.Right,$r.Bottom) }
function Mouse-Point([IntPtr]$Window,[uint32]$Message,[int]$X,[int]$Y,[long]$Flags=0) {
    [void][CaptureLongWin]::Send($Window,$Message,$Flags,($X -band 0xffff) -bor (($Y -band 0xffff) -shl 16))
}
function Snapshot([string]$Name) {
    $r=Window-Rect $script:panel; $f=Window-Rect $script:fixture.Hwnd
    $left=[Math]::Min($r.Left,$f.Left)-8; $top=[Math]::Min($r.Top,$f.Top)-8
    $right=[Math]::Max($r.Right,$f.Right)+8; $bottom=[Math]::Max($r.Bottom,$f.Bottom)+8
    $bitmap=[Drawing.Bitmap]::new($right-$left,$bottom-$top);$graphics=[Drawing.Graphics]::FromImage($bitmap)
    try { $graphics.CopyFromScreen($left,$top,0,0,$bitmap.Size);$bitmap.Save((Join-Path $runRoot $Name),[Drawing.Imaging.ImageFormat]::Png) }
    finally { $graphics.Dispose();$bitmap.Dispose() }
}
function Panel-Click([int]$Id) {
    $r=Window-Rect $script:panel; $scale=[int][CaptureLongWin]::GetDpiForWindow($script:panel)
    $cell=[int][Math]::Floor((36*$scale+48)/96);$pad=[int][Math]::Floor((4*$scale+48)/96)
    $onLeft=$script:placement -in @('right-outside','left-inside')
    $x=if($onLeft) {$pad+[int]($cell/2)} else {$r.Right-$r.Left-$pad-[int]($cell/2)}
    $y=$pad+($Id-201)*$cell+[int]($cell/2)
    Mouse-Point $script:panel 0x0201 $x $y 1
    if([CaptureLongWin]::IsWindow($script:panel)) { Mouse-Point $script:panel 0x0202 $x $y }
}
function UI-Evidence {
    $r=Window-Rect $script:panel;$scale=[int][CaptureLongWin]::GetDpiForWindow($script:panel)
    $cell=[int][Math]::Floor((36*$scale+48)/96);$pad=[int][Math]::Floor((4*$scale+48)/96)
    $left=if($script:placement -in @('right-outside','left-inside')) {$pad} else {$r.Right-$r.Left-$pad-$cell}
    $bitmap=[Drawing.Bitmap]::new($r.Right-$r.Left,$r.Bottom-$r.Top);$graphics=[Drawing.Graphics]::FromImage($bitmap)
    $edge=[Drawing.Bitmap]::new(1,1);$edgeGraphics=[Drawing.Graphics]::FromImage($edge)
    try {
        $graphics.CopyFromScreen($r.Left,$r.Top,0,0,$bitmap.Size)
        $edgeGraphics.CopyFromScreen($script:region.Left-1,[int](($script:region.Top+$script:region.Bottom)/2),0,0,$edge.Size)
        $edgeColor=$edge.GetPixel(0,0).ToArgb() -band 0xffffff;$ink=@()
        for($row=0;$row -lt 5;$row++) {
            $pixels=0;$expected=if($row -eq 0) {0x191b1e} else {0xf0eee8}
            for($y=$pad+$row*$cell+4;$y -lt $pad+($row+1)*$cell-4;$y++) {for($x=$left+4;$x -lt $left+$cell-4;$x++) {if(($bitmap.GetPixel($x,$y).ToArgb() -band 0xffffff) -eq $expected) {$pixels++}}}
            $ink+=$pixels
        }
        @{visible=($edgeColor -eq 0xf4a13b -and @($ink | Where-Object {$_ -gt 0}).Count -eq 5);frame_rgb=$edgeColor;icon_ink_pixels=$ink}
    } finally {$graphics.Dispose();$bitmap.Dispose();$edgeGraphics.Dispose();$edge.Dispose()}
}
function Check-VisibleUI([string]$Name) {
    Wait-For {$script:visibleEvidence=UI-Evidence;$script:visibleEvidence.visible} 'our frame and all icons remain visible above our topmost fixture' 4000
    Check $true ($Name+': screenshot border and all five icons stay visible above the topmost target') $script:visibleEvidence
}
function Start-Long([string]$Name,[int]$X,[int]$Width,[string]$ExpectedPlacement) {
    $script:fixture=[CaptureScrollFixture]::new($X,$script:fixtureY,$Width,$script:fixtureHeight)
    [void][CaptureLongWin]::SetWindowPos($script:fixture.Hwnd,[IntPtr]::Zero,0,0,0,0,3)
    [void][CaptureLongWin]::SetForegroundWindow($script:fixture.Hwnd);Start-Sleep -Milliseconds 200
    $dataRoot=Join-Path $runRoot ($Name+'-data');New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
    $script:currentDataRoot=$dataRoot
    @{include_cursor=$false;detect_ui=$false} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $dataRoot 'settings.json') -Encoding utf8NoBOM
    $script:gui=Start-Process -FilePath $Executable -ArgumentList @('--action','capture','--data-dir',('"'+$dataRoot+'"')) -WindowStyle Hidden -PassThru
    Wait-For { $script:overlay=@([CaptureLongWin]::Windows($script:gui.Id,'ptools.capture.view'))[0];$null -ne $script:overlay } 'our capture overlay'
    $screen=Window-Rect $script:overlay;$script:overlayBefore=Rect-Array $screen
    $f=Window-Rect $script:fixture.Hwnd
    $script:region=[CaptureLongWin+RECT]::new();$script:region.Left=$f.Left+8;$script:region.Top=$f.Top+8;$script:region.Right=$f.Right-8;$script:region.Bottom=$f.Bottom-8
    $x1=$script:region.Left-$screen.Left;$y1=$script:region.Top-$screen.Top;$x2=$script:region.Right-$screen.Left;$y2=$script:region.Bottom-$screen.Top
    Mouse-Point $script:overlay 0x0201 $x1 $y1 1;Mouse-Point $script:overlay 0x0200 $x2 $y2 1;Mouse-Point $script:overlay 0x0202 $x2 $y2
    [void][CaptureLongWin]::Send($script:overlay,0x0111,15)
    Wait-For { $script:panel=@([CaptureLongWin]::Windows($script:gui.Id,'ptools.capture.long-panel'))[0];$null -ne $script:panel } 'compact long capture icon panel'
    Start-Sleep -Milliseconds 200
    Check ([CaptureLongWin]::IsWindow($script:overlay) -and @([CaptureLongWin]::Windows($script:gui.Id,'ptools.capture.view')).Count -eq 1) ($Name+': entering long capture keeps the same screenshot window')
    Check (((Rect-Array (Window-Rect $script:overlay)) -join ',') -eq ($script:overlayBefore -join ',')) ($Name+': screenshot window bounds stay unchanged') $script:overlayBefore
    $ex=[CaptureLongWin]::GetWindowLongPtr($script:overlay,-20).ToInt64()
    Check (($ex -band 0x20) -ne 0 -and ($ex -band 0x80000) -ne 0) ($Name+': screenshot frame is transparent and passes input through') $ex
    Check (@([CaptureLongWin]::Children($script:panel) | Where-Object { $_ -eq 'Button' }).Count -eq 0) ($Name+': options are drawn icons without the old three text buttons')
    $r=Window-Rect $script:panel
    $script:placement=if($r.Left -ge $script:region.Right) {'right-outside'} elseif($r.Right -le $script:region.Left) {'left-outside'} elseif([Math]::Abs($r.Left-$script:region.Left) -lt [Math]::Abs($r.Right-$script:region.Right)) {'left-inside'} else {'right-inside'}
    Check ($script:placement -eq $ExpectedPlacement) ($Name+': panel chooses '+$ExpectedPlacement) @{region=Rect-Array $script:region;panel=Rect-Array $r}
    Check ($r.Left -ge $script:work.Left -and $r.Right -le $script:work.Right -and $r.Top -ge $script:work.Top -and $r.Bottom -le $script:work.Bottom) ($Name+': complete panel fits the monitor work area')
    Check-VisibleUI $Name
    Snapshot ($Name+'-initial.png')
}
function Finish-PrivateProcess([string]$Name) {
    Check ($script:gui.WaitForExit(5000)) ($Name+': finishing exits the private capture process')
    Check (-not [CaptureLongWin]::IsWindow($script:overlay) -and -not [CaptureLongWin]::IsWindow($script:panel)) ($Name+': original frame and icon panel are both destroyed')
    Check ([CaptureLongWin]::HotkeyFree(0,13) -and [CaptureLongWin]::HotkeyFree(0,27) -and [CaptureLongWin]::HotkeyFree(2,0x43) -and [CaptureLongWin]::HotkeyFree(2,0x54) -and [CaptureLongWin]::HotkeyFree(2,0x53)) ($Name+': long capture keyboard registrations are released')
    $script:gui.Dispose();$script:gui=$null;$script:fixture.Dispose();$script:fixture=$null
}
function Copy-And-Check([string]$Name,[int]$ExpectedExtraHeight,[scriptblock]$Action=$null) {
    if($script:clipboardReady) {
        $script:clipboardTouched=$true;Invoke-ClipboardWrite { [Windows.Forms.Clipboard]::SetText('private long capture sentinel '+[Guid]::NewGuid().ToString('N')) }
        if($Action) {& $Action} else {Panel-Click 201}
        Wait-For { [Windows.Forms.Clipboard]::ContainsImage() } 'our long capture image copied' 5000
        $image=[Windows.Forms.Clipboard]::GetImage();$bitmap=[Drawing.Bitmap]::new($image);$image.Dispose()
    } else {
        Panel-Click 202
        Wait-For { $script:exportPath=@(Get-ChildItem -LiteralPath (Join-Path $script:currentDataRoot 'images') -Filter '*.png' -ErrorAction SilentlyContinue | Where-Object { $_.Name -notlike '*-thumb.png' })[0];$null -ne $script:exportPath } 'our long capture stored in private history' 5000
        Wait-For { $script:finishedPin=@([CaptureLongWin]::Windows($script:gui.Id,'ptools.capture.view') | Where-Object { $_ -ne $script:overlay })[0];$null -ne $script:finishedPin } 'our completed long capture pin' 5000
        Check (-not [CaptureLongWin]::IsWindow($script:overlay) -and -not [CaptureLongWin]::IsWindow($script:panel)) ($Name+': finish-and-pin replaces the frame and its panel with the completed image')
        $bitmap=[Drawing.Bitmap]::new($script:exportPath.FullName)
    }
    try {
        $width=$script:region.Right-$script:region.Left;$height=$script:region.Bottom-$script:region.Top+$ExpectedExtraHeight
        Check ($bitmap.Width -eq $width -and $bitmap.Height -eq $height) ($Name+': output image has the expected stitched dimensions') @{actual=@($bitmap.Width,$bitmap.Height);expected=@($width,$height)}
        $lock=$bitmap.LockBits([Drawing.Rectangle]::new(0,0,$width,$height),[Drawing.Imaging.ImageLockMode]::ReadOnly,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try {$mismatches=[CaptureScrollFixture]::Mismatches($lock.Scan0,$lock.Stride,$width,$height,8,8)} finally {$bitmap.UnlockBits($lock)}
        Check ($mismatches -eq 0) ($Name+': every output pixel matches the fixture, with no frame, icons or thumbnail pollution') @{different_pixels=$mismatches;total_pixels=$width*$height}
        $bitmap.Save((Join-Path $runRoot ($Name+'-output.png')),[Drawing.Imaging.ImageFormat]::Png)
    } finally {$bitmap.Dispose()}
    if(-not $script:clipboardReady) {[void][CaptureLongWin]::PostMessage($script:finishedPin,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)}
    Finish-PrivateProcess $Name
}
function Keyboard-Finish([bool]$Cancel=$false) {
    foreach($modifier in @(0x10,0x11,0x12,0x5b,0x5c)) {if([CaptureLongWin]::GetAsyncKeyState($modifier) -lt 0) {throw 'A modifier is held; stopped the keyboard check to avoid interfering with user input'}}
    $key=if($Cancel) {27} else {13}
    if([CaptureLongWin]::GetAsyncKeyState($key) -lt 0) {throw 'The tested key is held; stopped the keyboard check to avoid interfering with user input'}
    [void][CaptureLongWin]::SetForegroundWindow($fixture.Hwnd)
    Check ([CaptureLongWin]::GetForegroundWindow() -eq $fixture.Hwnd) 'Keyboard check has focus on our private scrolling window'
    [CaptureLongWin]::Key($key)
}
try {
    if([Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') { throw 'Run with pwsh -STA -File scripts/verify-long-capture.ps1; no clipboard was changed' }
    $originalForeground=[CaptureLongWin]::GetForegroundWindow();$originalDpi=[CaptureLongWin]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $sequence=[CaptureLongWin]::GetClipboardSequenceNumber()
    try {
        $source=[Windows.Forms.Clipboard]::GetDataObject()
        $clipboardSnapshot=[Windows.Forms.DataObject]::new();$clipboardWasEmpty=$null -eq $source;$formats=@()
        if($source) {
            $formats=@($source.GetFormats($false));$clipboardWasEmpty=$formats.Count -eq 0
            foreach($format in $formats) {
                $value=$source.GetData($format,$false)
                if($value -is [Drawing.Image]) {$value=$value.Clone();$clipboardCopies.Add($value)}
                elseif($value -is [IO.Stream]) {
                    if(-not $value.CanSeek) {throw "Cannot safely snapshot nonseekable clipboard stream: $format"}
                    $stream=[IO.MemoryStream]::new();$position=$value.Position
                    try {$value.Position=0;$value.CopyTo($stream)} finally {$value.Position=$position}
                    $stream.Position=0;$value=$stream;$clipboardCopies.Add($value)
                } elseif($value -is [Array]) {$value=$value.Clone()}
                elseif($null -ne $value -and $value -isnot [string]) {throw "Unsupported clipboard format: $format"}
                if($null -eq $value) {throw "Cannot materialize clipboard format: $format"}
                $clipboardSnapshot.SetData($format,$false,$value)
            }
        }
        if([CaptureLongWin]::GetClipboardSequenceNumber() -ne $sequence) {throw 'Clipboard changed while snapshotting'}
        $clipboardReady=$true;$results.clipboard_formats=$formats;$results.output_path='Complete-and-copy with clipboard backup and restore'
    } catch {
        $results.clipboard_skip_reason=$_.Exception.Message;$results.output_path='Complete-and-pin; verify private history PNG; clipboard untouched'
        Write-Output ('INFO: '+$results.clipboard_skip_reason+'; using private history export without changing clipboard')
    }
    $work=[Windows.Forms.Screen]::PrimaryScreen.WorkingArea;$results.work_area=@($work.Left,$work.Top,$work.Right,$work.Bottom)
    $fixtureY=$work.Top+80;$fixtureHeight=[Math]::Min(460,$work.Height-160)
    if($work.Width -lt 800 -or $fixtureHeight -lt 240) {throw 'The private long capture fixture requires a work area of at least 800 by 400 physical pixels'}
    $narrow=[Math]::Min(720,[int]($work.Width/3))
    Start-Long 'paused' ($work.Left+32) $narrow 'right-outside'
    Panel-Click 204;$fixture.SetOffset(120);Start-Sleep -Milliseconds 1200;Snapshot 'paused-after-scroll.png'
    Copy-And-Check 'paused' 0
    Start-Long 'right-outside' ($work.Left+32) $narrow 'right-outside'
    Panel-Click 204;Snapshot 'right-outside-paused.png';Panel-Click 204
    foreach($offset in @(120,240,360)) {$fixture.SetOffset($offset);Start-Sleep -Milliseconds 1200}
    Check ([CaptureLongWin]::IsWindow($overlay) -and (((Rect-Array (Window-Rect $overlay)) -join ',') -eq ($overlayBefore -join ','))) 'Scrolling keeps the original frame and its screen bounds'
    Snapshot 'right-outside-stitched.png';Copy-And-Check 'right-outside' 360
    Start-Long 'left-outside' ($work.Right-$narrow-32) $narrow 'left-outside'
    $fixture.SetOffset(120);Start-Sleep -Milliseconds 1200;Snapshot 'left-outside-stitched.png';Copy-And-Check 'left-outside' 120
    Start-Long 'right-inside' ($work.Left+8) ($work.Width-16) 'right-inside'
    $fixture.SetOffset(120);Start-Sleep -Milliseconds 1200;Snapshot 'right-inside-stitched.png';Copy-And-Check 'right-inside' 120
    Start-Long 'save-cancel' ($work.Left+32) $narrow 'right-outside'
    [void][CaptureLongWin]::PostMessage($overlay,0x0111,[IntPtr]203,[IntPtr]::Zero)
    Wait-For {$script:saveDialog=@([CaptureLongWin]::Windows($gui.Id,'#32770'))[0];$null -ne $script:saveDialog} 'our long capture save dialog' 5000
    # The modal belongs to our private capture process. Cover our own viewport to
    # prove dialog pixels are excluded while capture sampling is suspended.
    [void][CaptureLongWin]::SetWindowPos($saveDialog,[IntPtr]::Zero,$region.Left+24,$region.Top+24,0,0,1)
    $fixture.SetOffset(120);Start-Sleep -Milliseconds 1200;Snapshot 'save-dialog-over-fixture.png'
    [void][CaptureLongWin]::PostMessage($saveDialog,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    Wait-For {@([CaptureLongWin]::Windows($gui.Id,'#32770')).Count -eq 0} 'cancelled save dialog closes' 5000
    Check ([CaptureLongWin]::IsWindow($overlay) -and [CaptureLongWin]::IsWindow($panel)) 'Cancelling Save keeps the original frame and panel alive'
    Start-Sleep -Milliseconds 1200;$fixture.SetOffset(240);Start-Sleep -Milliseconds 1200
    Check-VisibleUI 'save-cancel resumed'
    Snapshot 'save-cancel-resumed.png';Copy-And-Check 'save-cancel' 240
    Start-Long 'cancel' ($work.Left+32) $narrow 'right-outside'
    Panel-Click 205;Finish-PrivateProcess 'cancel'
    Start-Long 'keyboard' ($work.Left+32) $narrow 'right-outside'
    if($clipboardReady) {Copy-And-Check 'keyboard' 0 {Keyboard-Finish}}
    else {Keyboard-Finish $true;Finish-PrivateProcess 'keyboard';Check ([CaptureLongWin]::GetClipboardSequenceNumber() -eq $sequence) 'All long capture checks preserve the original clipboard sequence';$results.clipboard_preserved=$true}
    $results.status='passed'
} catch {$results.status='failed';$results.error=$_.Exception.Message;throw}
finally {
    if($gui) {$gui.Refresh();if(-not $gui.HasExited) {$gui.Kill($true);$gui.WaitForExit()};$gui.Dispose()}
    if($fixture) {$fixture.Dispose()}
    if($clipboardReady -and $clipboardTouched) {
        try {Invoke-ClipboardWrite {if($clipboardWasEmpty) {[Windows.Forms.Clipboard]::Clear()} else {[Windows.Forms.Clipboard]::SetDataObject($clipboardSnapshot,$true)}};$results.clipboard_restored=$true}
        catch {$results.clipboard_restored=$false;$results.clipboard_restore_error=$_.Exception.Message;$results.status='failed'}
    }
    foreach($copy in $clipboardCopies) {$copy.Dispose()}
    if($originalForeground -ne [IntPtr]::Zero -and [CaptureLongWin]::IsWindow($originalForeground)) {[void][CaptureLongWin]::SetForegroundWindow($originalForeground)}
    if($originalDpi -ne [IntPtr]::Zero) {[void][CaptureLongWin]::SetThreadDpiAwarenessContext($originalDpi)}
    $results | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $runRoot 'results.json') -Encoding utf8NoBOM
    Write-Output "Evidence: $runRoot"
    if($results.clipboard_restored -eq $false) {throw 'Could not restore the original clipboard; see results.json'}
}
