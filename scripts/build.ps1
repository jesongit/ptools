param([switch]$SkipTests, [string]$OutputRoot = '')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$distributionRoot = if ($OutputRoot) { [IO.Path]::GetFullPath($OutputRoot) } else { Join-Path $projectRoot 'dist' }
Push-Location $projectRoot
try {
    $metadata = cargo metadata --no-deps --format-version 1 --locked | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read package version' }
    $releaseVersion = ($metadata.packages | Where-Object name -eq 'ptools').version
    if (-not $SkipTests) {
        cargo test --workspace --locked
        if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
    }
    cargo build --workspace --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
    & (Join-Path $PSScriptRoot 'update-notices.ps1')
    $packageRoot = Join-Path $distributionRoot 'ptools'
    $pluginRoot = Join-Path $packageRoot 'plugins/applications'
    New-Item -ItemType Directory -Force -Path $pluginRoot | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'target/release/ptools.exe') -Destination $packageRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'target/release/ptools-applications.exe') -Destination $pluginRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'plugins/applications/plugin.json') -Destination $pluginRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'README.md') -Destination $packageRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/plugin-development.md') -Destination $packageRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'LICENSE') -Destination $packageRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'THIRD_PARTY_NOTICES.txt') -Destination $packageRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'THIRD_PARTY_NOTICES.txt') -Destination $pluginRoot -Force
    $captureRoot=Join-Path $packageRoot 'plugins/capture'
    New-Item -ItemType Directory -Force -Path $captureRoot | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'target/release/ptools-capture.exe') -Destination $captureRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'plugins/capture/plugin.json') -Destination $captureRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'THIRD_PARTY_NOTICES.txt') -Destination $captureRoot -Force
    $uninstallerRoot=Join-Path $packageRoot 'plugins/uninstaller'
    # Remove the retired bundled plugin from older staging directories. Resolve
    # the absolute target and verify the exact generated directory before deletion.
    if(Test-Path -LiteralPath $uninstallerRoot) {
        $resolvedPackageRoot=(Resolve-Path -LiteralPath $packageRoot).Path.TrimEnd([IO.Path]::DirectorySeparatorChar)
        $resolvedUninstallerRoot=(Resolve-Path -LiteralPath $uninstallerRoot).Path
        $expectedUninstallerRoot=[IO.Path]::GetFullPath((Join-Path $resolvedPackageRoot 'plugins/uninstaller'))
        if(-not $resolvedUninstallerRoot.Equals($expectedUninstallerRoot,[StringComparison]::OrdinalIgnoreCase) -or -not $resolvedUninstallerRoot.StartsWith($resolvedPackageRoot + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing to remove a directory outside the retired uninstaller package path' }
        foreach($directory in @($packageRoot,(Join-Path $packageRoot 'plugins'),$resolvedUninstallerRoot)) {
            $item=Get-Item -LiteralPath $directory
            if(-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Refusing to remove through a linked or non-directory package path' }
        }
        Remove-Item -LiteralPath $resolvedUninstallerRoot -Recurse -Force
    }
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/native-tools.md') -Destination $packageRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/windows-launcher.md') -Destination $packageRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/quick-input.md') -Destination $packageRoot -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/ui-style-guide.md') -Destination $packageRoot -Force
    $verification=Join-Path $projectRoot 'docs/verification.md'
    if(Test-Path -LiteralPath $verification) { Copy-Item -LiteralPath $verification -Destination $packageRoot -Force }
    $archive = Join-Path $distributionRoot "ptools-$releaseVersion-windows-x64.zip"
    # Explicit input paths keep user-created portable data out of distribution archives.
    $packageFiles=@((Join-Path $packageRoot 'ptools.exe'), (Join-Path $packageRoot 'plugins'), (Join-Path $packageRoot 'README.md'), (Join-Path $packageRoot 'plugin-development.md'), (Join-Path $packageRoot 'native-tools.md'), (Join-Path $packageRoot 'windows-launcher.md'), (Join-Path $packageRoot 'quick-input.md'), (Join-Path $packageRoot 'ui-style-guide.md'), (Join-Path $packageRoot 'LICENSE'), (Join-Path $packageRoot 'THIRD_PARTY_NOTICES.txt'))
    if(Test-Path -LiteralPath $verification) { $packageFiles+=(Join-Path $packageRoot 'verification.md') }
    Compress-Archive -Path $packageFiles -DestinationPath $archive -Force
    $pluginArchive = Join-Path $distributionRoot 'applications.zip'
    Compress-Archive -Path (Join-Path $pluginRoot '*') -DestinationPath $pluginArchive -Force
    $applicationsVersion=(Get-Content -LiteralPath (Join-Path $pluginRoot 'plugin.json') -Raw | ConvertFrom-Json).version
    $applicationsPackage=Join-Path $distributionRoot "applications-$applicationsVersion.ptplugin"
    Copy-Item -LiteralPath $pluginArchive -Destination $applicationsPackage -Force
    $checksumFiles=@($archive, $applicationsPackage)
    foreach($id in @('capture')) {
        $nativeVersion=(Get-Content -LiteralPath (Join-Path $packageRoot "plugins/$id/plugin.json") -Raw | ConvertFrom-Json).version
        $nativeArchive=Join-Path $distributionRoot "$id-$nativeVersion.zip"
        Compress-Archive -Path (Join-Path $packageRoot "plugins/$id/*") -DestinationPath $nativeArchive -Force
        $nativePlugin=Join-Path $distributionRoot "$id-$nativeVersion.ptplugin"
        Copy-Item -LiteralPath $nativeArchive -Destination $nativePlugin -Force
        $checksumFiles+=$nativePlugin
    }
    $checksums=$checksumFiles | ForEach-Object { '{0}  {1}' -f (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant(), (Split-Path -Leaf $_) }
    $checksums | Set-Content -LiteralPath (Join-Path $distributionRoot 'SHA256SUMS.txt') -Encoding ascii
    Write-Output "Ready: $packageRoot"
    Get-Item -LiteralPath $archive | Select-Object FullName, Length
} finally { Pop-Location }
