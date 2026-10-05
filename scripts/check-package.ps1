param([string]$DistributionRoot='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $DistributionRoot) { $DistributionRoot=Join-Path $projectRoot 'dist' }
$metadata=cargo metadata --manifest-path (Join-Path $projectRoot 'Cargo.toml') --no-deps --format-version 1 --locked | ConvertFrom-Json
if($LASTEXITCODE -ne 0) { throw 'Cannot read package version' }
$releaseVersion=($metadata.packages | Where-Object name -eq 'ptools').version
$archive=Join-Path $DistributionRoot "ptools-$releaseVersion-windows-x64.zip"
$destination=Join-Path $projectRoot ('artifacts/package-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
Expand-Archive -LiteralPath $archive -DestinationPath $destination
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
if(($doctor.plugins.id | Sort-Object) -join ',' -ne 'applications,capture') { throw 'Bundled plugin initialization failed' }
if(Test-Path -LiteralPath (Join-Path $destination 'plugins/uninstaller')) { throw 'Archive still contains the retired native uninstaller plugin' }
# Reproduce a returning user's initialized data directory with old tool programs.
$preferences=[ordered]@{hotkey='Ctrl+Alt+Space';disabled_plugins=@('capture','uninstaller');usage=@{'capture:history'=@{count=4;last_used=122};'uninstaller:open'=@{count=9;last_used=123}};plugin_hotkeys=@{'capture:capture'='Ctrl+Alt+Shift+1'}}
$settingsPath=Join-Path $dataRoot 'settings.json'
$preferences | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $settingsPath -Encoding utf8NoBOM
$preferencesHash=(Get-FileHash -LiteralPath $settingsPath -Algorithm SHA256).Hash
$fixtureData=Join-Path $dataRoot 'plugin-data/capture'
New-Item -ItemType Directory -Path $fixtureData -Force | Out-Null
'retained history fixture' | Set-Content -LiteralPath (Join-Path $fixtureData 'history-fixture.txt') -Encoding utf8NoBOM
$uninstallerData=Join-Path $dataRoot 'plugin-data/uninstaller'
New-Item -ItemType Directory -Path $uninstallerData -Force | Out-Null
$uninstallerFixture=Join-Path $uninstallerData 'history-fixture.txt'
'retained retired plugin data' | Set-Content -LiteralPath $uninstallerFixture -Encoding utf8NoBOM
$uninstallerFixtureHash=(Get-FileHash -LiteralPath $uninstallerFixture -Algorithm SHA256).Hash
foreach($id in @('applications','capture')) {
    $installedRoot=Join-Path $dataRoot "plugins/$id"
    $manifestPath=Join-Path $installedRoot 'plugin.json'
    $oldManifest=Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $oldManifest.version='0.1.0'
    $oldManifest | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $manifestPath -Encoding utf8NoBOM
    [IO.File]::WriteAllBytes((Join-Path $installedRoot $oldManifest.executable),[Text.Encoding]::UTF8.GetBytes('old plugin fixture'))
}
$oldUninstallerRoot=Join-Path $dataRoot 'plugins/uninstaller'
New-Item -ItemType Directory -Path $oldUninstallerRoot -Force | Out-Null
[ordered]@{schema_version=2;id='uninstaller';name='软件卸载';version='0.1.1';description='Legacy native uninstaller fixture';runtime='interactive';executable='ptools-uninstaller.exe';actions=@(@{id='open';title='软件卸载';keywords=@('uninstall')})} | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $oldUninstallerRoot 'plugin.json') -Encoding utf8NoBOM
[IO.File]::WriteAllBytes((Join-Path $oldUninstallerRoot 'ptools-uninstaller.exe'),[Text.Encoding]::UTF8.GetBytes('old uninstaller fixture'))
$doctor=Invoke-Packaged @('--doctor') | ConvertFrom-Json
foreach($id in @('applications','capture')) {
    $expected=(Get-FileHash -LiteralPath (Join-Path $destination "plugins/$id/ptools-$id.exe") -Algorithm SHA256).Hash
    $installed=(Get-FileHash -LiteralPath (Join-Path $dataRoot "plugins/$id/ptools-$id.exe") -Algorithm SHA256).Hash
    if($expected -ne $installed) { throw "Existing $id plugin did not upgrade with complete package" }
}
if(Test-Path -LiteralPath $oldUninstallerRoot) { throw 'Automatic upgrade retained the retired native uninstaller plugin' }
if($doctor.plugins.id -contains 'uninstaller') { throw 'Retired native uninstaller remains registered' }
if((Get-FileHash -LiteralPath $settingsPath -Algorithm SHA256).Hash -ne $preferencesHash) { throw 'Automatic upgrade changed user settings or disabled state' }
if((Get-Content -LiteralPath (Join-Path $fixtureData 'history-fixture.txt') -Raw).Trim() -ne 'retained history fixture') { throw 'Automatic upgrade changed plugin data' }
if((Get-FileHash -LiteralPath $uninstallerFixture -Algorithm SHA256).Hash -ne $uninstallerFixtureHash) { throw 'Retiring the old uninstaller changed its plugin data' }
Write-Output 'PASS: complete package upgrades application and capture plugins, removes the retired uninstaller and preserves preferences, disabled state, usage and plugin data'
foreach($id in @('capture')) {
    $null=Invoke-Packaged @('--uninstall',$id)
    $doctor=Invoke-Packaged @('--doctor') | ConvertFrom-Json
    if($doctor.plugins.id -contains $id) { throw "Uninstalled bundled plugin reappeared: $id" }
    $pluginVersion=(Get-Content -LiteralPath (Join-Path $destination "plugins/$id/plugin.json") -Raw | ConvertFrom-Json).version
    $null=Invoke-Packaged @('--install',(Join-Path $DistributionRoot "$id-$pluginVersion.ptplugin"))
}
$null=Invoke-Packaged @('--uninstall','applications')
$applicationsVersion=(Get-Content -LiteralPath (Join-Path $destination 'plugins/applications/plugin.json') -Raw | ConvertFrom-Json).version
$null=Invoke-Packaged @('--install',(Join-Path $DistributionRoot "applications-$applicationsVersion.ptplugin"))
$doctor=Invoke-Packaged @('--doctor') | ConvertFrom-Json
if($doctor.entry_count -le 0) { throw 'Standalone plugin package failed' }
if((Get-Content -LiteralPath (Join-Path $fixtureData 'history-fixture.txt') -Raw).Trim() -ne 'retained history fixture') { throw 'Plugin uninstall/reinstall changed capture history' }
if((Get-FileHash -LiteralPath $uninstallerFixture -Algorithm SHA256).Hash -ne $uninstallerFixtureHash) { throw 'Plugin uninstall/reinstall changed retired plugin data' }
if($doctor.plugins.id -contains 'uninstaller' -or (Test-Path -LiteralPath $oldUninstallerRoot)) { throw 'Retired native uninstaller reappeared after plugin management' }
foreach($query in @('设备管理器','shebeiguanliqi','sbglq','device manager','devmgmt.msc')) {
    $hits=@((Invoke-Packaged @('--search',$query)) | ConvertFrom-Json)
    $deviceHits=@($hits | Where-Object { $_.id -eq 'system:device-manager' -and $_.target.kind -eq 'system' -and $_.target.id -eq 'device-manager' })
    if($deviceHits.Count -ne 1) { throw "Standalone application plugin cannot search Windows Device Manager: $query" }
}
$displayHits=@((Invoke-Packaged @('--search','显示设置')) | ConvertFrom-Json | Where-Object { $_.target.kind -eq 'system' -and $_.target.id -eq 'display-settings' })
if($displayHits.Count -ne 1) { throw 'Standalone application plugin cannot search Windows display settings' }
$downloadHits=@((Invoke-Packaged @('--search','下载文件夹')) | ConvertFrom-Json | Where-Object { $_.target.kind -eq 'system' -and $_.target.id -eq 'downloads' })
if($downloadHits.Count -ne 1) { throw 'Standalone application plugin cannot search the Windows Downloads folder' }
if(@(Get-Process -Name ptools-applications -ErrorAction SilentlyContinue).Count -ne 0) { throw 'Packaged application plugin remains running after indexing' }
Write-Output 'PASS: standalone application package indexes Windows management, settings and folder entries and exits'
$sourceHash=(Get-FileHash -LiteralPath (Join-Path $projectRoot 'target/release/ptools.exe') -Algorithm SHA256).Hash
$packageHash=(Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash
if($sourceHash -ne $packageHash) { throw 'Archive contains a stale executable' }
foreach($id in @('applications','capture')) {
    $source=(Get-FileHash -LiteralPath (Join-Path $projectRoot "target/release/ptools-$id.exe") -Algorithm SHA256).Hash
    $packaged=(Get-FileHash -LiteralPath (Join-Path $destination "plugins/$id/ptools-$id.exe") -Algorithm SHA256).Hash
    if($source -ne $packaged) { throw "Archive contains a stale $id executable" }
}
[ordered]@{status='passed';archive=$archive;executable_sha256=$packageHash;entry_count=$doctor.entry_count;checks=@('Extracted executable runs','Both bundled plugins initialize','Complete package upgrades application and capture plugins','Automatic upgrade removes the retired native uninstaller plugin','Automatic upgrade preserves user settings, disabled state, usage and plugin data','Both standalone .ptplugin packages reinstall and index','Plugin uninstall/reinstall preserves capture history and retired plugin data','Retired uninstaller does not reappear after plugin management','Standalone application package searches Device Manager in Chinese, pinyin, initials, English and command aliases','Standalone application package searches Windows settings and known folders','Application plugin exits after indexing','Host, applications and capture executable hashes match release build')} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $destination 'results.json') -Encoding utf8NoBOM
Write-Output "PASS: extracted distribution and standalone plugin package ($($doctor.entry_count) applications)"
Write-Output "Evidence: $destination"
