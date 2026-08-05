# Run the complete verification boundary for ManT's Markdown-only Windows build.

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

function Invoke-Native {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][string]$Program,
        [Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments
    )

    Write-Host "`n==> $Label"
    Write-Host "`$ $Program $($Arguments -join ' ')"
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE"
    }
}

$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
$Packages = @(
    "--package", "mant-ast",
    "--package", "mant-core",
    "--package", "mant-ui",
    "--package", "mant"
)

Invoke-Native -Label "check Rust formatting" -Program "cargo" `
    -Arguments @("fmt", "--all", "--check")
Invoke-Native -Label "test portable Rust packages" -Program "cargo" `
    -Arguments (@("test", "--locked") + $Packages)
Invoke-Native -Label "lint portable Rust packages" -Program "cargo" `
    -Arguments (@("clippy", "--locked") + $Packages + @("--all-targets", "--", "-D", "warnings"))
Invoke-Native -Label "build release executable" -Program "cargo" `
    -Arguments @("build", "--locked", "--release", "--package", "mant")

$Mant = Join-Path $Root "target/release/mant.exe"
if (-not (Test-Path -PathType Leaf $Mant)) {
    throw "Cargo did not produce $Mant"
}

Write-Host "`n==> smoke-test release executable"
$Help = (& $Mant --help) -join "`n"
if ($LASTEXITCODE -ne 0 -or $Help -notmatch "mant <NAME\|MARKDOWN\|-> \[OPTIONS\]") {
    throw "release help smoke test failed"
}
$Query = (& $Mant README.md --format json --compact) -join "`n"
if ($LASTEXITCODE -ne 0 -or $Query -notmatch '"schema":"mant.query/v4"') {
    throw "release Markdown query smoke test failed"
}

Write-Host "`nWindows verification succeeded"
Write-Host "  executable: $Mant"
