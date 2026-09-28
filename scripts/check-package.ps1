$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$destination=Join-Path $projectRoot ('artifacts/package-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
Expand-Archive -LiteralPath (Join-Path $projectRoot 'dist/ptools-0.1.0-windows-x64.zip') -DestinationPath $destination
$executable=Join-Path $destination 'ptools.exe'
$dataRoot=Join-Path $destination 'test-data'
function Invoke-Packaged([string[]]$Arguments) {
    $info=[Diagnostics.ProcessStartInfo]::new($executable); $info.UseShellExecute=$false; $info.CreateNoWindow=$true; $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true
    $info.ArgumentList.Add('--data-dir'); $info.ArgumentList.Add($dataRoot)
    foreach($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    $process=[Diagnostics.Process]::Start($info); $output=$process.StandardOutput.ReadToEnd(); $errorText=$process.StandardError.ReadToEnd(); $process.WaitForExit()
    if($process.ExitCode -ne 0) { throw "Packaged command failed: $output $errorText" }
    return $output
}
$doctor=Invoke-Packaged @('--doctor') | ConvertFrom-Json
if($doctor.plugins.Count -ne 1 -or $doctor.plugins[0].id -ne 'applications') { throw 'Bundled plugin initialization failed' }
$null=Invoke-Packaged @('--uninstall','applications')
$null=Invoke-Packaged @('--install',(Join-Path $projectRoot 'dist/applications-0.1.0.ptplugin'))
$doctor=Invoke-Packaged @('--doctor') | ConvertFrom-Json
if($doctor.entry_count -le 0) { throw 'Standalone plugin package failed' }
$sourceHash=(Get-FileHash -LiteralPath (Join-Path $projectRoot 'target/release/ptools.exe') -Algorithm SHA256).Hash
$packageHash=(Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash
if($sourceHash -ne $packageHash) { throw 'Archive contains a stale executable' }
[ordered]@{status='passed';archive=(Join-Path $projectRoot 'dist/ptools-0.1.0-windows-x64.zip');executable_sha256=$packageHash;applications=$doctor.entry_count;checks=@('Extracted executable runs','Bundled plugin initializes','Standalone .ptplugin reinstalls and indexes','Executable hash matches release build')} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $destination 'results.json') -Encoding utf8NoBOM
Write-Output "PASS: extracted distribution and standalone plugin package ($($doctor.entry_count) applications)"
Write-Output "Evidence: $destination"
