#Requires -Version 7.0
param([string]$PackageRoot='', [string]$DataSource='')
$ErrorActionPreference='Stop'
$projectRoot=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$runRoot=Join-Path $projectRoot 'run'
$dataRoot=Join-Path $runRoot 'data'
if(-not $PackageRoot) { $PackageRoot=Join-Path $projectRoot 'dist/ptools' }
$PackageRoot=(Resolve-Path -LiteralPath $PackageRoot).Path
if(-not (Test-Path -LiteralPath (Join-Path $PackageRoot 'ptools.exe'))) { throw 'Build the complete package first' }
if(-not $DataSource) { $DataSource=Join-Path $env:APPDATA 'ptools' }
$DataSource=[IO.Path]::GetFullPath($DataSource).TrimEnd('\')
foreach($path in @($runRoot,$dataRoot,$DataSource,$PackageRoot)) {
    if((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "Linked paths are not supported: $path" }
}
if($PackageRoot.Equals($runRoot,[StringComparison]::OrdinalIgnoreCase)) { throw 'Package source must be separate from run/' }
$migrate=-not $DataSource.Equals($dataRoot,[StringComparison]::OrdinalIgnoreCase)
if($migrate -and @(Get-ChildItem -LiteralPath $dataRoot -Force -ErrorAction SilentlyContinue).Count) { throw 'run/data is not empty; reconcile its data before migration' }
$hostProcesses=@(Get-CimInstance Win32_Process -Filter "Name='ptools.exe'")
foreach($process in $hostProcesses) {
    if(-not $process.ExecutablePath -or -not $process.ExecutablePath.StartsWith($projectRoot+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'A ptools instance outside this project is running; select it explicitly before migration' }
}
$oldExe=if($hostProcesses.Count -eq 1) { $hostProcesses[0].ExecutablePath } else { $null }
$runKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$oldAutostart=(Get-ItemProperty -LiteralPath $runKey -ErrorAction SilentlyContinue).ptools
$evidence=Join-Path $projectRoot ('artifacts/migration-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force -Path $evidence,$runRoot,$dataRoot | Out-Null
$oldAutostart | Set-Content -LiteralPath (Join-Path $evidence 'previous-autostart.txt')
function File-Manifest([string]$Root) {
    @(Get-ChildItem -LiteralPath $Root -Recurse -File -Force | ForEach-Object {
        [pscustomobject]@{path=[IO.Path]::GetRelativePath($Root,$_.FullName);length=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash}
    })
}
try {
    # Stop only this project's host and tools belonging to the selected data.
    foreach($process in $hostProcesses) { Stop-Process -Id $process.ProcessId; Wait-Process -Id $process.ProcessId -Timeout 10 -ErrorAction SilentlyContinue }
    foreach($process in Get-CimInstance Win32_Process -Filter "Name='ptools-capture.exe' OR Name='ptools-applications.exe'") {
        if($process.ExecutablePath -and $process.ExecutablePath.StartsWith($DataSource+'\plugins\',[StringComparison]::OrdinalIgnoreCase)) {
            Stop-Process -Id $process.ProcessId; Wait-Process -Id $process.ProcessId -Timeout 10 -ErrorAction SilentlyContinue
        }
    }
    $before=File-Manifest $DataSource
    if($migrate) {
        foreach($item in Get-ChildItem -LiteralPath $DataSource -Force) { Copy-Item -LiteralPath $item.FullName -Destination $dataRoot -Recurse -Force }
        $after=File-Manifest $dataRoot
        if(@(Compare-Object ($before | ForEach-Object { "$($_.path)|$($_.length)|$($_.sha256)" }) ($after | ForEach-Object { "$($_.path)|$($_.length)|$($_.sha256)" })).Count) { throw 'Data migration hash verification failed' }
    }
    $before | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $evidence 'data-files.json')
    foreach($item in Get-ChildItem -LiteralPath $PackageRoot -Force) {
        if($item.Name -in @('data','portable.flag') -or $item.Name.StartsWith('.ptools-update-')) { continue }
        Copy-Item -LiteralPath $item.FullName -Destination $runRoot -Recurse -Force
    }
    [IO.File]::WriteAllText((Join-Path $runRoot 'portable.flag'),'')
    $executable=Join-Path $runRoot 'ptools.exe'
    $doctor=& $executable --data-dir $dataRoot --doctor | ConvertFrom-Json
    if($LASTEXITCODE -ne 0 -or $doctor.data_dir -ne $dataRoot) { throw 'Installed executable health check failed' }
    if($oldAutostart) {
        Set-ItemProperty -LiteralPath $runKey -Name ptools -Value ('"'+$executable+'" --hidden --data-dir "'+$dataRoot+'"')
    }
    Start-Process -FilePath $executable -ArgumentList @('--hidden','--data-dir',('"'+$dataRoot+'"')) -WorkingDirectory $runRoot -WindowStyle Hidden
    $live=$null
    for($attempt=0;$attempt -lt 20;$attempt++) {
        Start-Sleep -Milliseconds 250
        $live=@(Get-CimInstance Win32_Process -Filter "Name='ptools.exe'" | Where-Object ExecutablePath -eq $executable)
        if($live.Count -eq 1) { break }
    }
    if($live.Count -ne 1) { throw 'Installed host did not stay running' }
    $report=[ordered]@{version=$doctor.version;executable=$executable;data_dir=$dataRoot;source_data_retained=$DataSource;copied_files=$before.Count;verified_sha256=$true;autostart_updated=[bool]$oldAutostart;pid=$live[0].ProcessId}
    $report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidence 'result.json')
    $report | ConvertTo-Json
} catch {
    if($oldAutostart) { Set-ItemProperty -LiteralPath $runKey -Name ptools -Value $oldAutostart }
    if($oldExe -and -not (Get-Process ptools -ErrorAction SilentlyContinue)) {
        Start-Process -FilePath $oldExe -ArgumentList @('--hidden','--data-dir',('"'+$DataSource+'"')) -WindowStyle Hidden
    }
    throw
}
