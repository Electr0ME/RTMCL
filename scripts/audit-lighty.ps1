$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$auditRoot = Join-Path $projectRoot '.rtmcl/audit'
$sourceRoot = Join-Path $auditRoot 'source'
if (-not (Test-Path -LiteralPath (Join-Path $sourceRoot 'Cargo.toml'))) {
    New-Item -ItemType Directory -Force $sourceRoot | Out-Null
    $sourceArchive = Join-Path $auditRoot 'lighty-source.tar.gz'
    Invoke-WebRequest 'https://api.github.com/repos/Lighty-Launcher/LightyLauncherLib/tarball/94bd0553beeb89fa7868eb0e0a9f446ad85af793' -OutFile $sourceArchive
    tar -xf $sourceArchive -C $sourceRoot --strip-components=1
    if ($LASTEXITCODE -ne 0) { throw '源码归档解压失败' }
}
$packages = [ordered]@{
    'lighty-launcher' = ''
    'lighty-core' = 'crates/core'
    'lighty-auth' = 'crates/auth'
    'lighty-event' = 'crates/event'
    'lighty-java' = 'crates/java'
    'lighty-launch' = 'crates/launch'
    'lighty-loaders' = 'crates/loaders'
    'lighty-modsloader' = 'crates/modsloader'
    'lighty-version' = 'crates/version'
}
$records = foreach ($entry in $packages.GetEnumerator()) {
    $name = $entry.Key
    $version = '26.9.3'
    $metadata = Invoke-RestMethod "https://crates.io/api/v1/crates/$name/$version"
    $archive = Join-Path $auditRoot "$name-$version.crate"
    if (-not (Test-Path -LiteralPath $archive)) {
        Invoke-WebRequest "https://crates.io/api/v1/crates/$name/$version/download" -OutFile $archive
    }
    $checksum = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($checksum -ne $metadata.version.checksum) { throw "$name checksum mismatch" }
    tar -xf $archive -C $auditRoot
    if ($LASTEXITCODE -ne 0) { throw "$name extract failed" }
    $packageRoot = Join-Path $auditRoot "$name-$version"
    $upstreamRoot = if ($entry.Value) { Join-Path $sourceRoot $entry.Value } else { $sourceRoot }
    $vcs = Get-Content -LiteralPath (Join-Path $packageRoot '.cargo_vcs_info.json') -Raw | ConvertFrom-Json
    $differences = @()
    foreach ($file in Get-ChildItem -LiteralPath (Join-Path $packageRoot 'src') -File -Recurse) {
        $relative = [IO.Path]::GetRelativePath($packageRoot, $file.FullName)
        $upstream = Join-Path $upstreamRoot $relative
        if (-not (Test-Path -LiteralPath $upstream) -or
            (Get-FileHash -LiteralPath $upstream -Algorithm SHA256).Hash -ne
            (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash) {
            $differences += $relative.Replace('\', '/')
        }
    }
    $manifestMatches = (Get-FileHash -LiteralPath (Join-Path $packageRoot 'Cargo.toml.orig') -Algorithm SHA256).Hash -eq
        (Get-FileHash -LiteralPath (Join-Path $upstreamRoot 'Cargo.toml')).Hash
    [ordered]@{
        package = $name
        version = $version
        checksum = $checksum
        vcs_revision = $vcs.git.sha1
        rust_version = $metadata.version.rust_version
        source_differences = $differences
        original_manifest_matches = $manifestMatches
    }
    Write-Host "$name verified"
}
$records | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $projectRoot 'docs/lighty-release-audit.json') -Encoding utf8
