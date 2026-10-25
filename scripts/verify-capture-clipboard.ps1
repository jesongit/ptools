param([string]$Executable = '')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $Executable) { $Executable = Join-Path $projectRoot 'dist/ptools/plugins/capture/ptools-capture.exe' }
$Executable = (Resolve-Path -LiteralPath $Executable).Path
if ([Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') { throw 'Run with pwsh -STA; no clipboard was changed' }
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class ClipboardDiagnosis {
 public delegate bool EnumProc(IntPtr h,IntPtr l);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback,IntPtr l);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder name,int max);
 [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h,int id);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll",EntryPoint="SendMessageTimeoutW")] static extern IntPtr SendRaw(IntPtr h,uint msg,IntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
 [DllImport("user32.dll")] static extern bool OpenClipboard(IntPtr h);
 [DllImport("user32.dll")] static extern bool CloseClipboard();
 [DllImport("user32.dll")] static extern uint EnumClipboardFormats(uint previous);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClipboardFormatName(uint format,StringBuilder name,int max);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern uint RegisterClipboardFormat(string name);
 [DllImport("user32.dll",SetLastError=true)] static extern IntPtr GetClipboardData(uint format);
 [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
 [DllImport("user32.dll")] static extern IntPtr GetClipboardOwner();
 [DllImport("kernel32.dll")] static extern IntPtr GlobalLock(IntPtr memory);
 [DllImport("kernel32.dll")] static extern bool GlobalUnlock(IntPtr memory);
 [DllImport("kernel32.dll")] static extern UIntPtr GlobalSize(IntPtr memory);
 public static IntPtr[] Windows(uint pid) {
  var result=new List<IntPtr>();
  EnumWindows((h,l)=>{GetWindowThreadProcessId(h,out uint found);var name=new StringBuilder(256);GetClassName(h,name,256);if(found==pid&&name.ToString()=="ptools.capture.view")result.Add(h);return true;},IntPtr.Zero);
  return result.ToArray();
 }
 public static void Send(IntPtr h,uint msg,long w=0,long l=0) {
  if(SendRaw(h,msg,new IntPtr(w),new IntPtr(l),2,2000,out UIntPtr result)==IntPtr.Zero)throw new Exception("Window command timed out");
 }
 public static Dictionary<string,object> Probe(bool copy,IntPtr h) {
  if(copy)Send(h,0x111,9);
  var result=new Dictionary<string,object>();
  var watch=System.Diagnostics.Stopwatch.StartNew();
  while(!OpenClipboard(IntPtr.Zero)) {if(watch.ElapsedMilliseconds>3000)throw new Exception("Clipboard occupied");System.Threading.Thread.Sleep(1);}
  try {
   result["sequence"]=GetClipboardSequenceNumber();
   GetWindowThreadProcessId(GetClipboardOwner(),out uint pid);result["owner_pid"]=pid;
   var formats=new List<string>();uint format=0;
   while((format=EnumClipboardFormats(format))!=0) {var name=new StringBuilder(256);GetClipboardFormatName(format,name,256);formats.Add(format+":"+name);}
   result["formats"]=formats;
   uint pngFormat=RegisterClipboardFormat("PNG");
   result["png_format"]=pngFormat;
   IntPtr pngMemory=GetClipboardData(pngFormat);
   result["png_available"]=pngMemory!=IntPtr.Zero;
   if(pngMemory!=IntPtr.Zero) {
    ulong size=GlobalSize(pngMemory).ToUInt64();
    if(size<8||size>128*1024*1024)throw new Exception("Unexpected PNG clipboard size");
    IntPtr data=GlobalLock(pngMemory);
    if(data==IntPtr.Zero)throw new Exception("Cannot lock PNG clipboard");
    try{byte[] bytes=new byte[(int)size];Marshal.Copy(data,bytes,0,bytes.Length);result["png_payload"]=bytes;result["png_bytes"]=size;}
    finally{GlobalUnlock(pngMemory);}
   }
   foreach(uint f in new uint[]{17,8,2}) {
    IntPtr memory=GetClipboardData(f);
    result["format_"+f+"_available"]=memory!=IntPtr.Zero;
    if(memory==IntPtr.Zero) {result["format_"+f+"_error"]=Marshal.GetLastWin32Error();continue;}
    if(f==2)continue;
    ulong bytes=GlobalSize(memory).ToUInt64();result["format_"+f+"_bytes"]=bytes;
    if(bytes<40||bytes>128*1024*1024)throw new Exception("Unexpected DIB clipboard size");
    IntPtr data=GlobalLock(memory);
    if(data==IntPtr.Zero)throw new Exception("Cannot lock DIB clipboard");
    try {
     result["format_"+f+"_header"]=Marshal.ReadInt32(data);
     result["format_"+f+"_width"]=Marshal.ReadInt32(data,4);
     result["format_"+f+"_height"]=Marshal.ReadInt32(data,8);
     result["format_"+f+"_bpp"]=Marshal.ReadInt16(data,14);
     result["format_"+f+"_compression"]=Marshal.ReadInt32(data,16);
     result["format_"+f+"_image_size"]=Marshal.ReadInt32(data,20);
     byte[] payload=new byte[(int)bytes];Marshal.Copy(data,payload,0,payload.Length);result["format_"+f+"_payload"]=payload;
    }finally{GlobalUnlock(memory);}
   }
  }finally{CloseClipboard();}
  return result;
 }
}
'@
function Wait-For([scriptblock]$Condition) {
 $taskWatch=[Diagnostics.Stopwatch]::StartNew()
 while($taskWatch.ElapsedMilliseconds -lt 10000) { if(& $Condition) {return}; Start-Sleep -Milliseconds 40 }
 throw 'Window wait timed out'
}
$taskRun=Join-Path $projectRoot ('artifacts/capture-clipboard-'+(Get-Date -Format 'yyyyMMdd-HHmmss')+'-'+[Guid]::NewGuid().ToString('N').Substring(0,8))
$taskImages=Join-Path $taskRun 'data/images'
New-Item -ItemType Directory -Path $taskImages -Force | Out-Null
$taskCopies=[Collections.Generic.List[IDisposable]]::new()
$taskSnapshot=[Windows.Forms.DataObject]::new()
$taskSequence=[ClipboardDiagnosis]::GetClipboardSequenceNumber()
$taskOriginal=[Windows.Forms.Clipboard]::GetDataObject()
$taskWasEmpty=$null -eq $taskOriginal
if($taskOriginal) {
 $taskFormats=@($taskOriginal.GetFormats($false));$taskWasEmpty=$taskFormats.Count -eq 0
 foreach($taskFormat in $taskFormats) {
  $taskValue=$taskOriginal.GetData($taskFormat,$false)
  if($taskValue -is [Drawing.Image]) {$taskValue=$taskValue.Clone();$taskCopies.Add($taskValue)}
  elseif($taskValue -is [IO.Stream]) {
   if(-not $taskValue.CanSeek){throw 'Cannot snapshot clipboard stream'}
   $taskStream=[IO.MemoryStream]::new();$taskPosition=$taskValue.Position
   try{$taskValue.Position=0;$taskValue.CopyTo($taskStream)}finally{$taskValue.Position=$taskPosition}
   $taskStream.Position=0;$taskValue=$taskStream;$taskCopies.Add($taskValue)
  }elseif($taskValue -is [Array]){$taskValue=$taskValue.Clone()}
  elseif($null -ne $taskValue -and $taskValue -isnot [string]){throw ('Cannot snapshot clipboard format '+$taskFormat)}
  if($null -eq $taskValue){throw ('Cannot snapshot clipboard format '+$taskFormat)}
  $taskSnapshot.SetData($taskFormat,$false,$taskValue)
 }
}
if([ClipboardDiagnosis]::GetClipboardSequenceNumber() -ne $taskSequence){throw 'Clipboard changed before test'}
$taskForeground=[ClipboardDiagnosis]::GetForegroundWindow()
$taskGui=$null;$taskTouched=$false
$taskResult=[ordered]@{status='running';executable=$Executable;sha256=(Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash;checks=@();observations=@();clipboard_restored=$false;limits=@('Native clipboard and independent PNG/BMP readers are exercised; QQ chat input is not automated')}
function Check([bool]$Ok,[string]$Name) {
 $script:taskResult.checks+=@{name=$Name;passed=$Ok}
 if(-not $Ok){throw ('FAILED: '+$Name)}
 Write-Output ('PASS: '+$Name)
}
function Verify-Image-Probe($Probe,[string]$Phase) {
 # Retain only metadata in the report; all decoded pixels are our fixture.
 [byte[]]$taskPng=$Probe['png_payload'];[void]$Probe.Remove('png_payload')
 [byte[]]$taskDib=$Probe['format_8_payload'];[void]$Probe.Remove('format_8_payload')
 [byte[]]$taskDibV5=$Probe['format_17_payload'];[void]$Probe.Remove('format_17_payload')
 $script:taskResult.observations+=,$Probe
 Check ($Probe['png_available'] -and $taskPng.Length -ge 8) "$Phase exposes registered PNG"
 Check ($Probe['formats'][0] -eq ($Probe['png_format'].ToString()+':PNG')) "$Phase offers PNG first"
 Check ($Probe['format_17_height'] -eq 64 -and $Probe['format_17_image_size'] -eq 32768) "$Phase exposes a positive-height DIBV5 with explicit image size"
 Check ($Probe['format_8_available'] -and $Probe['format_8_height'] -eq 64 -and $Probe['format_2_available']) "$Phase exposes Windows CF_DIB and CF_BITMAP fallbacks"
 $taskPngStream=[IO.MemoryStream]::new($taskPng)
 $taskDecodedPng=$null;$taskDecodedBmp=$null;$taskBmpStream=$null
 try {
  $taskDecodedPng=[Drawing.Bitmap]::new($taskPngStream)
  Check ($taskDecodedPng.Width -eq 128 -and $taskDecodedPng.Height -eq 64) "$Phase PNG has the original dimensions"
  $taskBmpStream=[IO.MemoryStream]::new();$taskWriter=[IO.BinaryWriter]::new($taskBmpStream)
  $taskWriter.Write([byte[]]@(0x42,0x4D));$taskWriter.Write([uint32](14+$taskDib.Length));$taskWriter.Write([uint32]0)
  $taskHeader=[BitConverter]::ToInt32($taskDib,0)
  $taskExtraMasks=if($taskHeader -eq 40 -and [BitConverter]::ToInt32($taskDib,16) -eq 3){12}else{0}
  $taskWriter.Write([uint32](14+$taskHeader+$taskExtraMasks));$taskWriter.Write($taskDib);$taskWriter.Flush();$taskBmpStream.Position=0
  $taskDecodedBmp=[Drawing.Bitmap]::new($taskBmpStream)
  $taskPixelsMatch=$true;$taskFallbackMatch=$true
  foreach($taskY in 0..63) {foreach($taskX in 0..127) {
   $taskExpected=$script:taskExpectedImage.GetPixel($taskX,$taskY)
   if($taskDecodedPng.GetPixel($taskX,$taskY).ToArgb() -ne $taskExpected.ToArgb()){$taskPixelsMatch=$false}
   if($taskDecodedBmp.GetPixel($taskX,$taskY).ToArgb() -ne $taskExpected.ToArgb()){$taskFallbackMatch=$false}
  }}
  Check $taskPixelsMatch "$Phase PNG preserves every pixel, direction, color and alpha"
  Check $taskFallbackMatch "$Phase synthesized CF_DIB decodes in the correct direction and color"
 }finally{
  if($taskDecodedPng){$taskDecodedPng.Dispose()};if($taskDecodedBmp){$taskDecodedBmp.Dispose()}
  $taskPngStream.Dispose();if($taskBmpStream){$taskWriter.Dispose();$taskBmpStream.Dispose()}
 }
}
$taskExpectedImage=$null
try {
 $taskBitmap=[Drawing.Bitmap]::new(128,64)
 $taskGraphics=[Drawing.Graphics]::FromImage($taskBitmap)
 try{
  $taskGraphics.Clear([Drawing.Color]::Red)
  $taskGraphics.FillRectangle([Drawing.Brushes]::Blue,0,32,128,32)
  $taskBitmap.Save((Join-Path $taskImages '1-1.png'),[Drawing.Imaging.ImageFormat]::Png)
  $taskBitmap.Save((Join-Path $taskImages '1-1-thumb.png'),[Drawing.Imaging.ImageFormat]::Png)
 }finally{$taskGraphics.Dispose();$taskBitmap.Dispose()}
 $taskExpectedImage=[Drawing.Bitmap]::new((Join-Path $taskImages '1-1.png'))
 @(@{id='1-1';created=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds();source='Private clipboard fixture';width=128;height=64}) | ConvertTo-Json -AsArray | Set-Content -LiteralPath (Join-Path $taskImages 'index.json') -Encoding utf8NoBOM
 $taskData=Join-Path $taskRun 'data'
 $taskGui=Start-Process -FilePath $Executable -ArgumentList @('--action','history','--data-dir',('"'+$taskData+'"')) -WindowStyle Hidden -PassThru
 Wait-For {$script:taskHistory=@([ClipboardDiagnosis]::Windows($taskGui.Id) | Where-Object {[ClipboardDiagnosis]::GetDlgItem($_,1) -ne [IntPtr]::Zero})[0];$null -ne $script:taskHistory}
 [ClipboardDiagnosis]::Send($taskHistory,0x111,(1 -bor (2 -shl 16)))
 Wait-For {$script:taskPin=@([ClipboardDiagnosis]::Windows($taskGui.Id) | Where-Object {$_ -ne $taskHistory})[0];$null -ne $script:taskPin}
 $taskTouched=$true
 $taskProbe=[ClipboardDiagnosis]::Probe($true,$taskPin);$taskProbe['delay_ms']=0
 Verify-Image-Probe $taskProbe 'Immediate copy'
 $taskCopySequence=$taskProbe['sequence']
 foreach($taskDelay in @(200,1000,3000)) {
  Start-Sleep -Milliseconds $taskDelay
  $taskProbe=[ClipboardDiagnosis]::Probe($false,[IntPtr]::Zero);$taskProbe['delay_ms']=$taskDelay
  Verify-Image-Probe $taskProbe "After $taskDelay ms"
  Check ($taskProbe['sequence'] -eq $taskCopySequence) "After $taskDelay ms the clipboard has not been replaced"
 }
 $taskClipboardImage=[Windows.Forms.Clipboard]::GetImage()
 $taskResult.dotnet_image_read=$null -ne $taskClipboardImage
 Check $taskResult.dotnet_image_read 'Windows Forms reads the clipboard as an image'
 if($taskClipboardImage){$taskResult.dotnet_image_size=@($taskClipboardImage.Width,$taskClipboardImage.Height);$taskClipboardImage.Dispose()}
 foreach($taskWindow in [ClipboardDiagnosis]::Windows($taskGui.Id)){[ClipboardDiagnosis]::Send($taskWindow,0x10)}
 [void]$taskGui.WaitForExit(3000)
 $taskResult.process_exited=$taskGui.HasExited
 Check $taskResult.process_exited 'The private capture process exits cleanly'
 $taskProbe=[ClipboardDiagnosis]::Probe($false,[IntPtr]::Zero);$taskProbe['after_process_exit']=$true
 Verify-Image-Probe $taskProbe 'After capture process exit'
 $taskResult.status='passed'
}catch{
 $taskResult.status='failed'
 $taskResult.error=$_.Exception.Message
 throw
}finally{
 if($taskGui -and -not $taskGui.HasExited){Stop-Process -Id $taskGui.Id -Force}
 if($taskTouched){
  try{if($taskWasEmpty){[Windows.Forms.Clipboard]::Clear()}else{[Windows.Forms.Clipboard]::SetDataObject($taskSnapshot,$true)};$taskResult.clipboard_restored=$true}
  catch{$taskResult.status='failed';$taskResult.clipboard_restore_error=$_.Exception.Message}
 }
 foreach($taskCopy in $taskCopies){$taskCopy.Dispose()}
 if($taskExpectedImage){$taskExpectedImage.Dispose()}
 [void][ClipboardDiagnosis]::SetForegroundWindow($taskForeground)
 $taskResult | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $taskRun 'results.json')
 Write-Output ($taskResult.status+': '+$taskResult.checks.Count+' checks')
 Write-Output ('RESULT: '+(Join-Path $taskRun 'results.json'))
 if($taskTouched -and -not $taskResult.clipboard_restored){throw 'Could not restore original clipboard; see results.json'}
}
