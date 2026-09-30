param([switch]$SkipTests)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    if (-not $SkipTests) {
        cargo test --workspace --locked
        if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
    }
    cargo build --workspace --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
    & (Join-Path $PSScriptRoot 'update-notices.ps1')
    $packageRoot = Join-Path $projectRoot 'dist/ptools'
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
    foreach($id in @('capture','uninstaller')) {
        $nativeRoot=Join-Path $packageRoot "plugins/$id"
        New-Item -ItemType Directory -Force -Path $nativeRoot | Out-Null
        Copy-Item -LiteralPath (Join-Path $projectRoot "target/release/ptools-$id.exe") -Destination $nativeRoot -Force
        Copy-Item -LiteralPath (Join-Path $projectRoot "plugins/$id/plugin.json") -Destination $nativeRoot -Force
        Copy-Item -LiteralPath (Join-Path $projectRoot 'THIRD_PARTY_NOTICES.txt') -Destination $nativeRoot -Force
    }
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/native-tools.md') -Destination $packageRoot -Force
    $verification=Join-Path $projectRoot 'docs/verification.md'
    if(Test-Path -LiteralPath $verification) { Copy-Item -LiteralPath $verification -Destination $packageRoot -Force }
    $archive = Join-Path $projectRoot 'dist/ptools-0.1.0-windows-x64.zip'
    # Explicit input paths keep user-created portable data out of distribution archives.
    $packageFiles=@((Join-Path $packageRoot 'ptools.exe'), (Join-Path $packageRoot 'plugins'), (Join-Path $packageRoot 'README.md'), (Join-Path $packageRoot 'plugin-development.md'), (Join-Path $packageRoot 'native-tools.md'), (Join-Path $packageRoot 'LICENSE'), (Join-Path $packageRoot 'THIRD_PARTY_NOTICES.txt'))
    if(Test-Path -LiteralPath $verification) { $packageFiles+=(Join-Path $packageRoot 'verification.md') }
    Compress-Archive -Path $packageFiles -DestinationPath $archive -Force
    $pluginArchive = Join-Path $projectRoot 'dist/applications.zip'
    Compress-Archive -Path (Join-Path $pluginRoot '*') -DestinationPath $pluginArchive -Force
    Copy-Item -LiteralPath $pluginArchive -Destination (Join-Path $projectRoot 'dist/applications-0.1.0.ptplugin') -Force
    $checksumFiles=@($archive, (Join-Path $projectRoot 'dist/applications-0.1.0.ptplugin'))
    foreach($id in @('capture','uninstaller')) {
        $nativeArchive=Join-Path $projectRoot "dist/$id-0.1.0.zip"
        Compress-Archive -Path (Join-Path $packageRoot "plugins/$id/*") -DestinationPath $nativeArchive -Force
        $nativePlugin=Join-Path $projectRoot "dist/$id-0.1.0.ptplugin"
        Copy-Item -LiteralPath $nativeArchive -Destination $nativePlugin -Force
        $checksumFiles+=$nativePlugin
    }
    $checksums=$checksumFiles | ForEach-Object { '{0}  {1}' -f (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant(), (Split-Path -Leaf $_) }
    $checksums | Set-Content -LiteralPath (Join-Path $projectRoot 'dist/SHA256SUMS.txt') -Encoding ascii
    Write-Output "Ready: $packageRoot"
    Get-Item -LiteralPath $archive | Select-Object FullName, Length
} finally { Pop-Location }
