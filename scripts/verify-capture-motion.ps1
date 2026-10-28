param([string]$Executable='', [string]$BaselineExecutable='', [switch]$Daily)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if($Daily) { $Executable=Join-Path $projectRoot 'run/data/plugins/capture/ptools-capture.exe'; if($BaselineExecutable) {throw 'Daily mode cannot run a baseline instance'} }
elseif(-not $Executable) { $Executable=Join-Path $projectRoot 'dist/ptools/plugins/capture/ptools-capture.exe' }
$runRoot=Join-Path $projectRoot ('artifacts/capture-motion-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $runRoot -Force | Out-Null
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
public static class CaptureMotionWin {
  public delegate bool EnumProc(IntPtr h,IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb,IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder value,int max);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
  [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW")] static extern IntPtr SendRaw(IntPtr h,uint msg,IntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll")] static extern bool UpdateWindow(IntPtr h);
  [DllImport("gdi32.dll")] static extern bool GdiFlush();
  [DllImport("dwmapi.dll")] static extern int DwmFlush();
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  public static IntPtr Window(uint process) {
    IntPtr result=IntPtr.Zero;
    EnumWindows((h,l)=>{ GetWindowThreadProcessId(h,out uint pid);var name=new StringBuilder(256);GetClassName(h,name,256);
      if(pid==process && name.ToString()=="ptools.capture.view" && IsWindowVisible(h)) result=h;return true;},IntPtr.Zero);
    return result;
  }
  static void Send(IntPtr h,uint msg,int x,int y,int flags=0) {
    if(SendRaw(h,msg,new IntPtr(flags),new IntPtr(x|(y<<16)),2,5000,out UIntPtr result)==IntPtr.Zero)
      throw new Exception("Capture window stopped responding: "+msg);
  }
  [DllImport("user32.dll")] static extern short GetAsyncKeyState(int key);
  [DllImport("user32.dll")] static extern uint SendInput(uint count, INPUT[] input, int size);
  [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint Type; public UNION Data; }
  [StructLayout(LayoutKind.Explicit)] struct UNION { [FieldOffset(0)] public KEY Key; [FieldOffset(0)] public MOUSE Mouse; }
  [StructLayout(LayoutKind.Sequential)] struct KEY { public ushort Vk,Scan; public uint Flags,Time; public UIntPtr Extra; }
  [StructLayout(LayoutKind.Sequential)] struct MOUSE { public int X,Y; public uint Data,Flags,Time; public UIntPtr Extra; }
  public static void CaptureHotkey() {
    foreach(int key in new int[]{0x10,0x11,0x12,0x5B,0x5C}) if(GetAsyncKeyState(key)<0) throw new Exception("Modifier held; capture test stopped");
    var keys=new ushort[]{0x11,0x31,0x31,0x11}; var input=new INPUT[4];
    for(int i=0;i<4;i++) input[i]=new INPUT{Type=1,Data=new UNION{Key=new KEY{Vk=keys[i],Flags=i<2?0u:2u}}};
    if(SendInput(4,input,Marshal.SizeOf<INPUT>())!=4) {
      var release=new INPUT[2];
      release[0]=new INPUT{Type=1,Data=new UNION{Key=new KEY{Vk=0x31,Flags=2}}};
      release[1]=new INPUT{Type=1,Data=new UNION{Key=new KEY{Vk=0x11,Flags=2}}};
      SendInput(2,release,Marshal.SizeOf<INPUT>());throw new Exception("Capture hotkey injection failed");
    }
  }
  public static double[][] Measure(IntPtr h) {
    var paint=new List<double>();var composed=new List<double>();
    Send(h,0x201,200,200,1);
    for(int i=0;i<45;i++) {
      var timer=Stopwatch.StartNew();
      Send(h,0x200,800+i*12,600+i*6,1);UpdateWindow(h);GdiFlush();
      double painted=timer.Elapsed.TotalMilliseconds;
      if(DwmFlush()!=0) throw new Exception("DwmFlush failed");
      if(i>=5) {paint.Add(painted);composed.Add(timer.Elapsed.TotalMilliseconds);}
    }
    Send(h,0x202,1328,864);
    paint.Sort();composed.Sort();return new double[][]{paint.ToArray(),composed.ToArray()};
  }
}
'@
$reports=@();$completed=$false;$originalForeground=[CaptureMotionWin]::GetForegroundWindow()
$originalDpi=[CaptureMotionWin]::SetThreadDpiAwarenessContext([IntPtr](-4))
try {
    $candidates=@(); if($BaselineExecutable) {$candidates+=@{label='baseline';path=$BaselineExecutable}}
    $candidates+=@{label='candidate';path=$Executable}
    foreach($candidate in $candidates) {
        $resolved=(Resolve-Path -LiteralPath $candidate.path).Path
        $dataRoot=Join-Path $runRoot $candidate.label
        New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
        if($Daily) {
            if(@(Get-CimInstance Win32_Process -Filter "Name='ptools-capture.exe'" | Where-Object ExecutablePath -eq $resolved).Count -gt 0) {throw 'Daily capture is already open; no test input sent'}
            $settings=Get-Content -LiteralPath (Join-Path $projectRoot 'run/data/settings.json') -Raw | ConvertFrom-Json -AsHashtable
            if($settings.disabled_plugins -contains 'capture' -or ($null -ne $settings.plugin_hotkeys -and $settings.plugin_hotkeys.ContainsKey('capture:capture') -and $settings.plugin_hotkeys['capture:capture'] -ne 'Ctrl+1')) {throw 'Daily Ctrl+1 mapping is not the default; no test input sent'}
            [CaptureMotionWin]::CaptureHotkey()
            $timer=[Diagnostics.Stopwatch]::StartNew();$launched=@()
            while($launched.Count -eq 0 -and $timer.ElapsedMilliseconds -lt 15000) {
                $launched=@(Get-CimInstance Win32_Process -Filter "Name='ptools-capture.exe'" | Where-Object ExecutablePath -eq $resolved);Start-Sleep -Milliseconds 50
            }
            if($launched.Count -ne 1) {throw 'Ctrl+1 did not start the daily capture plugin'}
            $process=Get-Process -Id $launched[0].ProcessId
        } else {
            '{"detect_ui":false,"include_cursor":false}' | Set-Content -LiteralPath (Join-Path $dataRoot 'settings.json')
            $process=Start-Process -FilePath $resolved -ArgumentList @('--action','capture','--data-dir',('"'+$dataRoot+'"')) -WindowStyle Hidden -PassThru
        }
        $window=[IntPtr]::Zero
        try {
            $timer=[Diagnostics.Stopwatch]::StartNew()
            while($window -eq [IntPtr]::Zero -and $timer.ElapsedMilliseconds -lt 15000) {
                $window=[CaptureMotionWin]::Window($process.Id);Start-Sleep -Milliseconds 40
            }
            if($window -eq [IntPtr]::Zero) {throw 'Capture overlay did not open'}
            $r=[CaptureMotionWin+RECT]::new();[void][CaptureMotionWin]::GetWindowRect($window,[ref]$r)
            Start-Sleep -Milliseconds 150
            $samples=[CaptureMotionWin]::Measure($window)
            $reports+=@{label=$candidate.label;daily=[bool]$Daily;executable=$resolved;sha256=(Get-FileHash -LiteralPath $resolved).Hash;
                size=@(($r.Right-$r.Left),($r.Bottom-$r.Top));frames=$samples[0].Length;
                paintMedianMs=$samples[0][20];paintP95Ms=$samples[0][38];composedMedianMs=$samples[1][20];composedP95Ms=$samples[1][38]}
        } finally {
            if($window -ne [IntPtr]::Zero) {[void][CaptureMotionWin]::PostMessage($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)}
            if(-not $process.WaitForExit(5000)) {$process.Kill($true);$process.WaitForExit()}
            $process.Dispose()
        }
    }
    $completed=$true
} finally {
    if($originalForeground -ne [IntPtr]::Zero -and [CaptureMotionWin]::IsWindow($originalForeground)) {[void][CaptureMotionWin]::SetForegroundWindow($originalForeground)}
    [void][CaptureMotionWin]::SetThreadDpiAwarenessContext($originalDpi)
    @{status=if($completed){'measured'}else{'failed'};input='native mouse messages into visible isolated capture windows; UpdateWindow, GdiFlush, DwmFlush';limits='Does not inject physical mouse movement or measure hardware-to-display latency';reports=$reports} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $runRoot 'results.json')
    Write-Output "Evidence: $runRoot"
}
$reports | ConvertTo-Json -Depth 5
