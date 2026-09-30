$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$metadata=cargo metadata --format-version 1 --locked --filter-platform x86_64-pc-windows-gnu | ConvertFrom-Json
if($LASTEXITCODE -ne 0) { throw 'Dependency metadata failed' }
$resolved=@{}; foreach($node in $metadata.resolve.nodes) { $resolved[$node.id]=$true }
$lines=[Collections.Generic.List[string]]::new()
$lines.Add('ptools v0.1.0 - third-party notices')
$lines.Add('License texts from the locked Windows x64 GNU dependencies, including build dependencies.')
foreach($package in ($metadata.packages | Where-Object { $_.source -and $resolved.ContainsKey($_.id) } | Sort-Object name,version)) {
    $directory=Split-Path -Parent $package.manifest_path
    $licenses=@(Get-ChildItem -LiteralPath $directory -File | Where-Object { $_.Name -match '^(LICENSE|COPYING|UNLICENSE)' } | Sort-Object Name)
    if($package.license_file) {
        $declared=Join-Path $directory $package.license_file
        if(Test-Path -LiteralPath $declared -PathType Leaf) { $licenses+=Get-Item -LiteralPath $declared }
    }
    if($licenses.Count -eq 0) { throw "No license text found: $($package.name)" }
    $lines.Add(''); $lines.Add('============================================================')
    $lines.Add("$($package.name) $($package.version) - $($package.license)")
    $lines.Add("https://crates.io/crates/$($package.name)/$($package.version)")
    foreach($file in ($licenses | Sort-Object FullName -Unique)) {
        $lines.Add(''); $lines.Add("--- $($file.Name) ---")
        $license=[IO.File]::ReadAllText($file.FullName).Replace("`r`n","`n")
        $lines.Add([regex]::Replace($license,'(?m)[\t ]+$',''))
    }
}
$destination=Join-Path $projectRoot 'THIRD_PARTY_NOTICES.txt'
$temporary=Join-Path $projectRoot 'THIRD_PARTY_NOTICES.txt.tmp'
[IO.File]::WriteAllText($temporary, ($lines -join "`n"), [Text.UTF8Encoding]::new($false))
[IO.File]::Move($temporary,$destination,$true)
