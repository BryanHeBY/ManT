# Run the complete verification boundary for ManT's native Windows build.

param(
    [ValidateSet("debug", "release")]
    [string]$BuildProfile = "release"
)

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
    "--package", "libmandoc-rs",
    "--package", "mant-ir",
    "--package", "mant-protocol",
    "--package", "mant-sources",
    "--package", "mant-engine",
    "--package", "mant-ui",
    "--package", "mant"
)

$InstallerTokens = $null
$InstallerErrors = $null
Write-Host "`n==> check Windows installer syntax"
[Management.Automation.Language.Parser]::ParseFile(
    (Join-Path $Root "scripts/install.ps1"),
    [ref]$InstallerTokens,
    [ref]$InstallerErrors
) | Out-Null
if ($InstallerErrors.Count -ne 0) {
    throw "Windows installer syntax check failed: $($InstallerErrors -join '; ')"
}

Write-Host "`n==> test Windows installer receipt uninstall"
$InstallerTestRoot = Join-Path ([IO.Path]::GetTempPath()) "mant-installer-$([guid]::NewGuid().ToString('N'))"
$PreviousLocalAppData = $env:LOCALAPPDATA
$PreviousAppData = $env:APPDATA
try {
    $env:LOCALAPPDATA = Join-Path $InstallerTestRoot "local"
    $env:APPDATA = Join-Path $InstallerTestRoot "roaming"
    $InstallerState = Join-Path $env:LOCALAPPDATA "ManT"
    $InstallerBin = Join-Path $InstallerTestRoot "bin"
    $InstallerDocuments = Join-Path $InstallerTestRoot "documents"
    New-Item $InstallerState -ItemType Directory -Force | Out-Null
    New-Item $InstallerBin -ItemType Directory -Force | Out-Null
    New-Item $InstallerDocuments -ItemType Directory -Force | Out-Null
    $InstallerBinary = Join-Path $InstallerBin "mant.exe"
    $InstallerManual = Join-Path $InstallerDocuments "mant.md"
    $UserDocument = Join-Path $InstallerDocuments "user.md"
    New-Item $InstallerBinary -ItemType File | Out-Null
    New-Item $InstallerManual -ItemType File | Out-Null
    New-Item $UserDocument -ItemType File | Out-Null
    [ordered]@{
        schema = "mant.install/v1"
        version = "0.5.0"
        installDir = $InstallerBin
        dataDir = $InstallerDocuments
        binary = $InstallerBinary
        manual = $InstallerManual
        pathAdded = $false
    } | ConvertTo-Json | Set-Content (Join-Path $InstallerState "install-receipt.json") -Encoding UTF8

    & (Join-Path $Root "scripts/install.ps1") -Uninstall
    if ((Test-Path $InstallerBinary) -or (Test-Path $InstallerManual)) {
        throw "Windows uninstaller retained an installer-owned file"
    }
    if (-not (Test-Path $UserDocument) -or -not (Test-Path $InstallerDocuments)) {
        throw "Windows uninstaller removed a user-owned path"
    }
} finally {
    $env:LOCALAPPDATA = $PreviousLocalAppData
    $env:APPDATA = $PreviousAppData
    Remove-Item $InstallerTestRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Invoke-Native -Label "check Rust formatting" -Program "cargo" `
    -Arguments @("fmt", "--all", "--check")
Invoke-Native -Label "test portable Rust packages" -Program "cargo" `
    -Arguments (@("test", "--locked") + $Packages)
Invoke-Native -Label "lint portable Rust packages" -Program "cargo" `
    -Arguments (@("clippy", "--locked") + $Packages + @("--all-targets", "--", "-D", "warnings"))
& (Join-Path $PSScriptRoot "build-and-smoke.ps1") -BuildProfile $BuildProfile

Write-Host "`nWindows verification succeeded"
