# Package an already-built native ManT executable for Windows.

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

function Fail([string]$Message) {
    throw "release packaging failed: $Message"
}

$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$Cargo = Get-Content (Join-Path $Root "Cargo.toml") -Raw
$Workspace = [regex]::Match($Cargo, '(?ms)^\[workspace\.package\]\s*(.*?)(?=^\[|\z)')
$VersionMatch = [regex]::Match($Workspace.Groups[1].Value, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $VersionMatch.Success) {
    Fail "Cargo.toml has no workspace package version"
}
$Version = $VersionMatch.Groups[1].Value

if ($env:MANT_RELEASE_TAG) {
    if ($env:MANT_RELEASE_TAG -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$') {
        Fail "release tag '$($env:MANT_RELEASE_TAG)' must use the form vMAJOR.MINOR.PATCH"
    }
    if ($env:MANT_RELEASE_TAG.Substring(1) -ne $Version) {
        Fail "release tag $($env:MANT_RELEASE_TAG) does not match workspace version $Version"
    }
}

if (-not [Environment]::Is64BitOperatingSystem) {
    Fail "public Windows archives require a 64-bit host"
}
$Target = "windows-x64"
if ($env:MANT_RELEASE_TARGET -and $env:MANT_RELEASE_TARGET -ne $Target) {
    Fail "release runner target mismatch: expected $($env:MANT_RELEASE_TARGET), built $Target"
}

$Binary = if ($env:MANT_BINARY) { $env:MANT_BINARY } else { Join-Path $Root "target/release/mant.exe" }
if (-not (Test-Path -PathType Leaf $Binary)) {
    Fail "missing executable $Binary; run cargo build --release -p mant first"
}

$ArchiveRoot = "mant-$Version-$Target"
$Dist = Join-Path $Root "dist"
$Staging = Join-Path $Dist ".release-staging"
$Package = Join-Path $Staging $ArchiveRoot
$Archive = Join-Path $Dist "$ArchiveRoot.zip"

Remove-Item $Staging -Recurse -Force -ErrorAction SilentlyContinue
New-Item $Dist -ItemType Directory -Force | Out-Null
New-Item $Package -ItemType Directory -Force | Out-Null
New-Item (Join-Path $Package "LICENSES") -ItemType Directory -Force | Out-Null
Copy-Item $Binary (Join-Path $Package "mant.exe")
Copy-Item (Join-Path $Root "docs/manuals/mant.md") (Join-Path $Package "mant.md")
Copy-Item (Join-Path $Root "README.md") (Join-Path $Package "README.md")
Copy-Item (Join-Path $Root "LICENSE") (Join-Path $Package "LICENSE")
Copy-Item `
    (Join-Path $Root "crates/libmandoc-rs/vendor/mandoc-1.14.6/LICENSE") `
    (Join-Path $Package "LICENSES/mandoc.txt")

Remove-Item $Archive -Force -ErrorAction SilentlyContinue
Compress-Archive -Path $Package -DestinationPath $Archive -CompressionLevel Optimal
$Hash = (Get-FileHash $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
$Checksum = "$Hash  $([IO.Path]::GetFileName($Archive))`n"
[IO.File]::WriteAllText("$Archive.sha256", $Checksum, [Text.UTF8Encoding]::new($false))
Remove-Item $Staging -Recurse -Force

Write-Host "packaged $Archive"
Write-Host $Checksum.TrimEnd()
