# Build one Windows product profile and smoke-test the resulting executable.

param(
    [ValidateSet("debug", "release")]
    [string]$BuildProfile = "release",
    [switch]$Annotated
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$Root = if ($env:MANT_WORKSPACE) {
    $env:MANT_WORKSPACE
} else {
    Split-Path -Parent $PSScriptRoot
}
Set-Location $Root
$env:LIBMANDOC_RS_DENY_WARNINGS = "1"

$CargoArguments = @("build", "--locked", "--package", "mant")
if ($BuildProfile -eq "release") {
    $CargoArguments = @("build", "--locked", "--release", "--package", "mant")
}
if ($Annotated) {
    $CargoArguments += @("--features", "annotated-preview")
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
    $Query -notmatch '"schema":"mant.query/v0.12"' -or
    $Query -notmatch '"schema":"mant.document/v0.12"'
) {
    throw "$BuildProfile Markdown query smoke test failed"
}
if ($Annotated) {
    # Pinned CVS man_term.c::pre_TP prints this exact fixture's HEAD/BODY.
    $Fixture = Join-Path $Root "tests/fixtures/roff/annotated-fixed-mentions.1"
    $Body = (& $Mant --annotated-preview --input $Fixture --input-format roff `
        --format text --display direct --color never) -join "`n"
    if ($LASTEXITCODE -ne 0 -or -not $Body.Contains("own --foo body")) {
        throw "$BuildProfile annotated roff smoke test failed"
    }
}

Write-Host "`nproduct build succeeded"
Write-Host "  executable: $Mant"
