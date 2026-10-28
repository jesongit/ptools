#Requires -Version 7.0
[CmdletBinding(SupportsShouldProcess)]
param(
    [ValidateSet('lan','github','both')][string]$Source='lan',
    [string]$DistributionRoot='',
    [string]$LanBase='http://192.168.31.10:13000',
    [string]$LanRepository='posase/ptools',
    [string]$GithubRepository='jesongit/ptools',
    [string]$NotesFile='',
    [switch]$Publish
)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $DistributionRoot) { $DistributionRoot=Join-Path $projectRoot 'dist' }
$metadata=cargo metadata --manifest-path (Join-Path $projectRoot 'Cargo.toml') --no-deps --locked --format-version 1 | ConvertFrom-Json
if($LASTEXITCODE -ne 0) { throw 'Cannot read package version' }
$version=($metadata.packages | Where-Object name -eq 'ptools').version
$tag="v$version"
$archive="ptools-$version-windows-x64.zip"
$checksums=Join-Path $DistributionRoot 'SHA256SUMS.txt'
$files=[Collections.Generic.List[string]]::new()
foreach($line in Get-Content -LiteralPath $checksums) {
    if($line -notmatch '^([a-fA-F0-9]{64})  ([a-zA-Z0-9._-]+)$') { throw 'Invalid checksum line' }
    $hash=$Matches[1]; $name=$Matches[2]
    $file=Join-Path $DistributionRoot $name
    if((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $hash) { throw "Checksum mismatch: $name" }
    if($files.Contains($name)) { throw "Duplicate checksum entry: $name" }
    $files.Add($name)
}
if(-not $files.Contains($archive)) { throw "Missing full application archive: $archive" }
$files.Add('SHA256SUMS.txt')
$notes=if($NotesFile) { Get-Content -LiteralPath $NotesFile -Raw } else { "ptools $version Windows x64 complete package. Download $archive; source archives are not executable packages." }
$sources=if($Source -eq 'both') { @('lan','github') } else { @($Source) }
foreach($provider in $sources) {
    $repository=if($provider -eq 'lan') { $LanRepository } else { $GithubRepository }
    $base=if($provider -eq 'lan') { "$($LanBase.TrimEnd('/'))/api/v1" } else { 'https://api.github.com' }
    if(-not $PSCmdlet.ShouldProcess("$base/repos/$repository", "Upload $tag assets: $($files -join ', '); publish=$Publish")) { continue }
    # Only publish a clean, tagged commit, so the release really matches Git.
    $dirty=git -C $projectRoot status --porcelain --untracked-files=normal
    if($LASTEXITCODE -ne 0 -or $dirty) { throw 'Commit changes before publishing a release' }
    $head=git -C $projectRoot rev-parse HEAD
    $tagCommit=git -C $projectRoot rev-parse "$tag^{commit}" 2>$null
    if($LASTEXITCODE -ne 0 -or $tagCommit -ne $head) { throw "Create $tag at HEAD and push it to $provider before publishing" }
    $remoteTag=git -C $projectRoot ls-remote --tags $(if($provider -eq 'lan'){'lan'}else{'origin'}) "refs/tags/$tag" "refs/tags/$tag^{}"
    if($LASTEXITCODE -ne 0 -or -not ($remoteTag -match "^$head\s")) { throw "Push $tag to $provider before publishing" }
    $token=if($provider -eq 'lan') {
        if($env:FORGEJO_TOKEN) { $env:FORGEJO_TOKEN } elseif($env:PTOOLS_UPDATE_TOKEN) { $env:PTOOLS_UPDATE_TOKEN }
        else {
            $tokenFile=if($env:PTOOLS_UPDATE_TOKEN_FILE) { $env:PTOOLS_UPDATE_TOKEN_FILE } else { Join-Path $env:USERPROFILE '.ssh/.env' }
            if(Test-Path -LiteralPath $tokenFile) {
                foreach($line in (Get-Content -LiteralPath $tokenFile)) {
                    if($line -match '^\s*(?:export\s+)?(?:PTOOLS[-_]UPDATE[-_]TOKEN|FORGEJO[-_]TOKEN|GITEA[-_]TOKEN|TOKEN|令牌)\s*[=:：]\s*(\S+)\s*$') {
                        $Matches[1].Trim([char]34,[char]39); break
                    } elseif($line.Trim() -match '^[a-fA-F0-9]{40}$') { $line.Trim(); break }
                }
            }
        }
    } else {
        if($env:GH_TOKEN) { $env:GH_TOKEN } elseif($env:GITHUB_TOKEN) { $env:GITHUB_TOKEN } else { gh auth token 2>$null }
    }
    if(-not $token) { throw "Missing API token for $provider. SSH authenticates Git, not the Release API." }
    $headers=@{Authorization="$(if($provider -eq 'lan'){'token'}else{'Bearer'}) $token";Accept='application/json';'User-Agent'='ptools-release'}
    $releaseUrl="$base/repos/$repository/releases"
    $release=$null
    try { $release=Invoke-RestMethod -Uri "$releaseUrl/tags/$tag" -Headers $headers }
    catch { if([int]$_.Exception.Response.StatusCode -ne 404) { throw "Cannot read $provider release (HTTP $([int]$_.Exception.Response.StatusCode))" } }
    if(-not $release) {
        $body=@{tag_name=$tag;target_commitish=$head;name=$tag;body=$notes;draft=$true;prerelease=$false} | ConvertTo-Json
        $release=Invoke-RestMethod -Method Post -Uri $releaseUrl -Headers $headers -ContentType 'application/json' -Body $body
    }
    foreach($name in $files) {
        $file=Join-Path $DistributionRoot $name
        $existing=@($release.assets | Where-Object name -eq $name)
        if($existing.Count -gt 0) {
            # Never silently overwrite an existing release artifact.
            $asset=$existing[0]
            $download=if($provider -eq 'github') { $asset.url } else { $asset.browser_download_url }
            $downloadHeaders=$headers.Clone(); $downloadHeaders.Accept='application/octet-stream'
            $tempFile=Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString()+'.ptools-release-verify')
            try {
                Invoke-WebRequest -Uri $download -Headers $downloadHeaders -OutFile $tempFile
                if((Get-FileHash -LiteralPath $tempFile).Hash -ne (Get-FileHash -LiteralPath $file).Hash) { throw "Existing asset differs: $name. Use a new version." }
            } finally { if(Test-Path -LiteralPath $tempFile) { Remove-Item -LiteralPath $tempFile } }
            continue
        }
        if($provider -eq 'lan') {
            $null=Invoke-RestMethod -Method Post -Uri "$releaseUrl/$($release.id)/assets?name=$([uri]::EscapeDataString($name))" -Headers $headers -Form @{attachment=Get-Item -LiteralPath $file}
        } else {
            $upload=$release.upload_url -replace '\{.*$',''
            $null=Invoke-RestMethod -Method Post -Uri "$($upload)?name=$([uri]::EscapeDataString($name))" -Headers $headers -ContentType 'application/octet-stream' -InFile $file
        }
    }
    $verified=Invoke-RestMethod -Uri "$releaseUrl/$($release.id)" -Headers $headers
    foreach($name in $files) {
        $asset=@($verified.assets | Where-Object name -eq $name)
        if($asset.Count -ne 1 -or $asset[0].size -ne (Get-Item -LiteralPath (Join-Path $DistributionRoot $name)).Length) { throw "Uploaded attachment verification failed: $name" }
    }
    if($Publish) {
        $verified=Invoke-RestMethod -Method Patch -Uri "$releaseUrl/$($release.id)" -Headers $headers -ContentType 'application/json' -Body (@{draft=$false;body=$notes} | ConvertTo-Json)
    }
    [pscustomobject]@{source=$provider;tag=$tag;draft=$verified.draft;assets=$files -join ', ';url=$verified.html_url}
}
