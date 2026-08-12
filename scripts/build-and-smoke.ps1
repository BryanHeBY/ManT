# Build one Windows product profile and smoke-test the resulting executable.

param(
    [ValidateSet("debug", "release")]
    [string]$BuildProfile = "release"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$Root = if ($env:MANT_WORKSPACE) {
    $env:MANT_WORKSPACE
} else {
    Split-Path -Parent $PSScriptRoot
}
Set-Location $Root

$CargoArguments = @("build", "--locked", "--package", "mant")
if ($BuildProfile -eq "release") {
    $CargoArguments = @("build", "--locked", "--release", "--package", "mant")
}

Write-Host "`n==> build $BuildProfile executable"
Write-Host "`$ cargo $($CargoArguments -join ' ')"
& cargo @CargoArguments
if ($LASTEXITCODE -ne 0) {
    throw "$BuildProfile build failed with exit code $LASTEXITCODE"
}

$Mant = Join-Path $Root "target/$BuildProfile/mant.exe"
if (-not (Test-Path -PathType Leaf $Mant)) {
    throw "Cargo did not produce $Mant"
}

Write-Host "`n==> smoke-test $BuildProfile executable"
$Help = (& $Mant --help) -join "`n"
if ($LASTEXITCODE -ne 0 -or $Help -notmatch "mant <SELECTOR> \[OPTIONS\]") {
    throw "$BuildProfile help smoke test failed"
}
$Query = (& $Mant --input README.md --format json --compact) -join "`n"
if (
    $LASTEXITCODE -ne 0 -or
    $Query -notmatch '"schema":"mant.query/v7"' -or
    $Query -notmatch '"schema":"mant.document/v7"'
) {
    throw "$BuildProfile Markdown query smoke test failed"
}

Write-Host "`nproduct build succeeded"
Write-Host "  executable: $Mant"
