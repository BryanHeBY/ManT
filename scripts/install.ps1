# Install, update, or uninstall ManT for one Windows user.

param(
    [switch]$Update,
    [switch]$Uninstall,
    [string]$Version,
    [string]$InstallDir,
    [string]$DataDir,
    [switch]$NoManual,
    [switch]$NoModifyPath,
    [switch]$Force,
    [Alias("h")][switch]$Help
)

& {
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$ReceiptSchema = "mant.install/v1"
$BundledManuals = @("mant.md", "mant-ir.md", "mant-markdown.md", "mant-protocol.md", "mant-roff.md")

function Fail([string]$Message) {
    throw "mant installer: $Message"
}

function Assert-GitHubAttestation([string]$Artifact) {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
        return
    }
    try {
        & gh auth status *> $null
    } catch {
        return
    }
    if ($LASTEXITCODE -ne 0) {
        return
    }
    try {
        & gh attestation verify $Artifact --repo $Repository *> $null
    } catch {
        Fail "GitHub attestation verification failed for $(Split-Path -Leaf $Artifact)"
    }
    if ($LASTEXITCODE -ne 0) {
        Fail "GitHub attestation verification failed for $(Split-Path -Leaf $Artifact)"
    }
    Write-Host "Verified GitHub provenance for $(Split-Path -Leaf $Artifact)"
}

function Show-Usage {
    Write-Host @"
Install, update, or uninstall ManT.

Usage:
  install.ps1 [options]

Options:
  -Update                 Explicit alias for the default install/update action
  -Uninstall              Remove files owned by the one-line installer
  -Version VERSION        Install a specific release instead of latest
  -InstallDir DIRECTORY   Override the executable directory
  -DataDir DIRECTORY      Override the registered-document directory
  -NoManual               Do not install the bundled ManT manuals
  -NoModifyPath           Do not add the executable directory to user PATH
  -Force                  Reinstall even when the selected version is current
  -Help                   Show this help

MANT_VERSION, MANT_INSTALL_DIR, and MANT_DATA_DIR provide the same overrides.
"@
}

function Normalize-PathEntry([string]$Path) {
    if (-not $Path) {
        return ""
    }
    return $Path.Trim().TrimEnd('\')
}

function Test-PathEntry([string]$Left, [string]$Right) {
    return (Normalize-PathEntry $Left) -ieq (Normalize-PathEntry $Right)
}

function Add-UserPath([string]$Directory) {
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $Entries = @($UserPath -split ';' | Where-Object { $_ })
    if ($Entries | Where-Object { Test-PathEntry $_ $Directory }) {
        return $false
    }

    $NewUserPath = if ($UserPath) {
        "$Directory;$($UserPath.TrimEnd(';'))"
    } else {
        $Directory
    }
    [Environment]::SetEnvironmentVariable("Path", $NewUserPath, "User")
    return $true
}

function Add-ProcessPath([string]$Directory) {
    $Entries = @($env:Path -split ';' | Where-Object { $_ })
    if (-not ($Entries | Where-Object { Test-PathEntry $_ $Directory })) {
        $env:Path = "$Directory;$env:Path"
    }
}

function Remove-PathEntry([string]$Directory) {
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $UserEntries = @($UserPath -split ';' | Where-Object {
        $_ -and -not (Test-PathEntry $_ $Directory)
    })
    [Environment]::SetEnvironmentVariable("Path", ($UserEntries -join ';'), "User")

    $ProcessEntries = @($env:Path -split ';' | Where-Object {
        $_ -and -not (Test-PathEntry $_ $Directory)
    })
    $env:Path = $ProcessEntries -join ';'
}

function Test-AbsolutePath([string]$Path) {
    return $Path -and [IO.Path]::IsPathRooted($Path) -and ($Path -match '^[A-Za-z]:[\\/]' -or $Path -match '^[\\/]{2}[^\\/]+[\\/][^\\/]+')
}

function Validate-AbsolutePath([string]$Path, [string]$Label) {
    if (-not (Test-AbsolutePath $Path)) {
        Fail "$Label must be an absolute path"
    }
    if ($Path -match '[\x00-\x1f\x7f]') { Fail "$Label contains a control character" }
}

function Get-InstalledVersion([string]$Binary) {
    if (-not (Test-Path -PathType Leaf $Binary)) {
        return $null
    }
    try {
        $Output = & $Binary --version 2>$null
        if ($LASTEXITCODE -eq 0 -and $Output -match '^mant\s+(\S+)') {
            return $Matches[1]
        }
    } catch {
        return $null
    }
    return $null
}

function Write-Receipt(
    [string]$Path,
    [string]$InstalledVersion,
    [string]$ExecutableDirectory,
    [string]$DocumentDirectory,
    [string]$Binary,
    [string[]]$Manuals,
    [bool]$PathAdded
) {
    $ReceiptDirectory = Split-Path -Parent $Path
    New-Item $ReceiptDirectory -ItemType Directory -Force | Out-Null
    $TemporaryReceipt = "$Path.$PID.tmp"
    [ordered]@{
        schema = $ReceiptSchema
        version = $InstalledVersion
        installDir = $ExecutableDirectory
        dataDir = $DocumentDirectory
        binary = $Binary
        manuals = @($Manuals)
        pathAdded = $PathAdded
        layout = $(if ($InstalledVersion -match '^0\.([0-9]|10|11)\.') { "legacy" } else { "unix-v1" })
        dataBinding = $DataBinding
    } | ConvertTo-Json | Set-Content $TemporaryReceipt -Encoding UTF8
    Move-Item $TemporaryReceipt $Path -Force
}

if ($Help) {
    Show-Usage
    return
}
if ($Uninstall -and $Update) {
    Fail "-Uninstall cannot be combined with -Update"
}
if (-not [Environment]::Is64BitOperatingSystem) {
    Fail "public Windows releases require a 64-bit host"
}
$MantUserHome = if (Test-AbsolutePath $env:HOME) { $env:HOME } else { $env:USERPROFILE }
Validate-AbsolutePath $MantUserHome "user home"
$DefaultInstallDir = Join-Path $MantUserHome ".local\bin"
$StateBase = if (Test-AbsolutePath $env:XDG_STATE_HOME) { $env:XDG_STATE_HOME } else { Join-Path $MantUserHome ".local\state" }
Validate-AbsolutePath $StateBase "installer state directory"
$LegacyRoot = if ($env:APPDATA) { Join-Path $env:APPDATA "ManT" } else { $null }
$LegacyInstallDir = if ($env:LOCALAPPDATA) { Join-Path $env:LOCALAPPDATA "Programs\ManT\bin" } else { $null }
$ExplicitDataDir = [bool]($DataDir -or $env:MANT_DATA_DIR)

# Windows PowerShell 5.1 can otherwise negotiate an obsolete TLS version.
if ($PSVersionTable.PSEdition -eq "Desktop") {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
}

$Repository = "BryanHeBY/ManT"
$GitHub = "https://github.com/$Repository"
$ReceiptPath = Join-Path $StateBase "mant\install-receipt.json"
$ReceiptReadPath = $ReceiptPath
if (-not (Test-Path -LiteralPath $ReceiptPath) -and $env:LOCALAPPDATA) {
    $LegacyReceipt = Join-Path $env:LOCALAPPDATA "ManT\install-receipt.json"
    if (Test-Path -LiteralPath $LegacyReceipt -PathType Leaf) { $ReceiptReadPath = $LegacyReceipt }
}
$Receipt = $null
if (Test-Path -LiteralPath $ReceiptReadPath -PathType Leaf) {
    try {
        $Receipt = Get-Content -LiteralPath $ReceiptReadPath -Raw | ConvertFrom-Json
    } catch {
        Fail "could not read installer receipt: $($_.Exception.Message)"
    }
    if ($Receipt.schema -ne $ReceiptSchema) {
        Fail "installer receipt has an unsupported schema"
    }
}

if ($Uninstall) {
    if (-not $Receipt) {
        Fail "no installer receipt was found; ManT was not installed by this script"
    }
    Validate-AbsolutePath $Receipt.installDir "receipt install directory"
    Validate-AbsolutePath $Receipt.dataDir "receipt data directory"
    Validate-AbsolutePath $Receipt.binary "receipt binary path"
    $ExpectedBinary = Join-Path $Receipt.installDir "mant.exe"
    if (-not (Test-PathEntry $Receipt.binary $ExpectedBinary)) {
        Fail "installer receipt contains an invalid binary path"
    }
    $ReceiptManuals = if ($Receipt.PSObject.Properties.Name -contains "manuals") {
        @($Receipt.manuals)
    } elseif ($Receipt.manual) {
        @([string]$Receipt.manual)
    } else {
        @()
    }
    foreach ($ReceiptManual in $ReceiptManuals) {
        Validate-AbsolutePath $ReceiptManual "receipt manual path"
        $ManualName = Split-Path -Leaf $ReceiptManual
        $ExpectedManual = Join-Path $Receipt.dataDir $ManualName
        if ($ManualName -notin $BundledManuals -or -not (Test-PathEntry $ReceiptManual $ExpectedManual)) {
            Fail "installer receipt contains an invalid manual path"
        }
    }

    $Removed = $false
    if (Test-Path -PathType Leaf $Receipt.binary) {
        Remove-Item $Receipt.binary -Force
        Write-Host "Removed $($Receipt.binary)"
        $Removed = $true
    }
    foreach ($ReceiptManual in $ReceiptManuals) {
        if (Test-Path -PathType Leaf $ReceiptManual) {
            Remove-Item $ReceiptManual -Force
            Write-Host "Removed $ReceiptManual"
            $Removed = $true
        }
    }
    if ($Receipt.pathAdded -and -not (Test-PathEntry $Receipt.installDir $DefaultInstallDir)) {
        Remove-PathEntry $Receipt.installDir
        Write-Host "Removed $($Receipt.installDir) from user PATH"
    }
    Remove-Item -LiteralPath $ReceiptReadPath -Force

    if ($Removed) {
        Write-Host "Uninstalled ManT $($Receipt.version)"
    } else {
        Write-Host "ManT files were already absent; removed the installer receipt."
    }
    return
}

if (-not $Version) {
    $Version = $env:MANT_VERSION
}
if (-not $InstallDir) {
    $InstallDir = if ($env:MANT_INSTALL_DIR) {
        $env:MANT_INSTALL_DIR
    } elseif ($Receipt -and -not (Test-PathEntry $Receipt.installDir $LegacyInstallDir)) {
        $Receipt.installDir
    } else {
        $DefaultInstallDir
    }
}
if (-not $DataDir) {
    $DataDir = if ($env:MANT_DATA_DIR) {
        $env:MANT_DATA_DIR
    } elseif ($Receipt) {
        $Receipt.dataDir
    } else {
        $DataBase = if ($env:MANT_DATA_HOME) { $env:MANT_DATA_HOME } elseif (Test-AbsolutePath $env:XDG_DATA_HOME) { Join-Path $env:XDG_DATA_HOME "mant" } else { Join-Path $MantUserHome ".local\share\mant" }
        Join-Path $DataBase "documents"
    }
}
Validate-AbsolutePath $InstallDir "install directory"
Validate-AbsolutePath $DataDir "data directory"
$LegacyDocuments = if ($LegacyRoot) { Join-Path $LegacyRoot "documents" } else { $null }
$DataBinding = if (-not $ExplicitDataDir -and (-not $Receipt -or $Receipt.dataBinding -eq "runtime" -or (Test-PathEntry $Receipt.dataDir $LegacyDocuments))) { "runtime" } else { "custom" }

if ($Version) {
    $Tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
} else {
    $Release = Invoke-RestMethod `
        -Uri "https://api.github.com/repos/$Repository/releases/latest" `
        -Headers @{ Accept = "application/vnd.github+json" }
    $Tag = [string]$Release.tag_name
}
if ($Tag -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$') {
    Fail "release tag '$Tag' is not a supported version"
}

$Version = $Tag.Substring(1)
$ManualNames = if ($Version -match '^0\.[0-6]\.\d+') { @("mant.md") } else { $BundledManuals }
$Target = "windows-x64"
$Archive = "mant-$Version-$Target.zip"
$ReleaseUrl = "$GitHub/releases/download/$Tag"
$BinaryPath = Join-Path $InstallDir "mant.exe"
$CurrentVersion = Get-InstalledVersion $BinaryPath
if (-not $CurrentVersion -and $Receipt) { $CurrentVersion = Get-InstalledVersion $Receipt.binary }

function Resolve-ReleaseLayout([string]$LayoutBinary) {
    if ($Version -match '^0\.([0-9]|10|11)\.') {
        if (-not $ExplicitDataDir -and -not $Receipt -and $LegacyRoot) { return (Join-Path $LegacyRoot "documents") }
        return $DataDir
    }
    if ($LegacyRoot -and (-not $Receipt -or $Receipt.layout -ne "unix-v1")) {
        & $LayoutBinary --installer-migrate $LegacyRoot | Out-Null
        if ($LASTEXITCODE -ne 0) { Fail "storage migration failed; the previous binary and original data were retained" }
    }
    $RuntimeDocuments = & $LayoutBinary --installer-paths documents
    if ($LASTEXITCODE -ne 0) { Fail "could not resolve the new runtime document directory" }
    Validate-AbsolutePath $RuntimeDocuments "runtime document directory"
    $LegacyDocuments = if ($LegacyRoot) { Join-Path $LegacyRoot "documents" } else { $null }
    if ($DataBinding -eq "runtime") { return $RuntimeDocuments }
    if (-not (Test-PathEntry $DataDir $RuntimeDocuments)) { Write-Warning "custom manual destination $DataDir differs from runtime $RuntimeDocuments; configure document discovery accordingly" }
    return $DataDir
}
function Get-RetainedManuals {
    @($ReceiptManuals | ForEach-Object {
        $Name = Split-Path -Leaf $_
        if ($Name -in $BundledManuals -and (Test-PathEntry $_ (Join-Path $Receipt.dataDir $Name))) {
            $Relocated = Join-Path $DataDir $Name
            if (Test-Path -LiteralPath $Relocated -PathType Leaf) { $Relocated }
        }
    })
}
$ReceiptManuals = if ($Receipt -and $Receipt.PSObject.Properties.Name -contains "manuals") {
    @($Receipt.manuals)
} elseif ($Receipt -and $Receipt.manual) {
    @([string]$Receipt.manual)
} else {
    @()
}
$OwnedManuals = if (-not $NoManual) {
    @($ManualNames | ForEach-Object { Join-Path $DataDir $_ })
} elseif ($Receipt -and (Test-PathEntry $Receipt.dataDir $DataDir)) {
    $ReceiptManuals
} else {
    @()
}
$MissingManuals = @($ManualNames | Where-Object {
    -not (Test-Path -PathType Leaf (Join-Path $DataDir $_))
})
$ManualReady = $NoManual -or $MissingManuals.Count -eq 0
$PathAdded = [bool]($Receipt -and $Receipt.pathAdded -and (Test-PathEntry $Receipt.installDir $InstallDir))

if ($CurrentVersion -eq $Version -and (Test-Path -LiteralPath $BinaryPath -PathType Leaf)) {
    $DataDir = Resolve-ReleaseLayout $BinaryPath
    $OwnedManuals = if (-not $NoManual) { @($ManualNames | ForEach-Object { Join-Path $DataDir $_ }) } else { @(Get-RetainedManuals) }
    $ManualReady = $NoManual -or @($ManualNames | Where-Object { -not (Test-Path -LiteralPath (Join-Path $DataDir $_) -PathType Leaf) }).Count -eq 0
}

if (-not $Force -and $CurrentVersion -eq $Version -and $ManualReady) {
    Write-Receipt $ReceiptPath $Version $InstallDir $DataDir $BinaryPath $OwnedManuals $PathAdded
    if (-not $NoModifyPath) {
        try {
            if (Add-UserPath $InstallDir) {
                $PathAdded = $true
                Write-Receipt $ReceiptPath $Version $InstallDir $DataDir $BinaryPath $OwnedManuals $PathAdded
            }
            Add-ProcessPath $InstallDir
        } catch {
            Write-Warning "could not update PATH: $($_.Exception.Message)"
        }
    }
    Write-Host "ManT $Version is already up to date."
    return
}

$Temporary = Join-Path ([IO.Path]::GetTempPath()) "mant-install-$([guid]::NewGuid().ToString('N'))"
try {
    New-Item $Temporary -ItemType Directory -Force | Out-Null
    $ArchivePath = Join-Path $Temporary $Archive
    $ChecksumsPath = Join-Path $Temporary "SHA256SUMS"
    Invoke-WebRequest -UseBasicParsing -Uri "$ReleaseUrl/$Archive" -OutFile $ArchivePath
    Invoke-WebRequest -UseBasicParsing -Uri "$ReleaseUrl/SHA256SUMS" -OutFile $ChecksumsPath

    $ChecksumText = Get-Content $ChecksumsPath -Raw
    $ChecksumPattern = "(?m)^([0-9a-fA-F]{64})\s+\*?$([regex]::Escape($Archive))\r?$"
    $ChecksumMatch = [regex]::Match($ChecksumText, $ChecksumPattern)
    if (-not $ChecksumMatch.Success) {
        Fail "SHA256SUMS does not contain $Archive"
    }
    $Expected = $ChecksumMatch.Groups[1].Value
    $Actual = (Get-FileHash $ArchivePath -Algorithm SHA256).Hash
    if ($Actual -ine $Expected) {
        Fail "SHA-256 verification failed for $Archive"
    }
    Assert-GitHubAttestation $ArchivePath

    $Expanded = Join-Path $Temporary "expanded"
    Expand-Archive -Path $ArchivePath -DestinationPath $Expanded
    $Package = Join-Path $Expanded "mant-$Version-$Target"
    $Binary = Join-Path $Package "mant.exe"
    if (-not (Test-Path -PathType Leaf $Binary)) {
        Fail "$Archive does not contain mant.exe"
    }
    $ManualDirectory = Join-Path $Package "manuals"
    if (-not (Test-Path -PathType Leaf (Join-Path $ManualDirectory "manifest.txt"))) {
        $ManualDirectory = $Package
        $ManualNames = @("mant.md")
    }
    if (-not (Test-Path -PathType Leaf (Join-Path $ManualDirectory "mant.md"))) {
        Fail "$Archive does not contain the ManT manuals"
    }

    foreach ($ManualName in $ManualNames) {
        if (-not $NoManual -and -not (Test-Path -LiteralPath (Join-Path $ManualDirectory $ManualName) -PathType Leaf)) { Fail "manual bundle is missing $ManualName" }
    }
    $DataDir = Resolve-ReleaseLayout $Binary
    New-Item $InstallDir -ItemType Directory -Force | Out-Null
    if (-not $NoManual) {
        New-Item $DataDir -ItemType Directory -Force | Out-Null
        $OwnedManuals = @()
        foreach ($ManualName in $ManualNames) {
            $Manual = Join-Path $ManualDirectory $ManualName
            if (-not (Test-Path -PathType Leaf $Manual)) {
                Fail "manual bundle is missing $ManualName"
            }
            $ManualPath = Join-Path $DataDir $ManualName
            Copy-Item $Manual $ManualPath -Force
            $OwnedManuals += $ManualPath
        }
    } else {
        $OwnedManuals = @(Get-RetainedManuals)
    }
    $StagedBinary = Join-Path $InstallDir ".mant-install-$PID.exe"
    Copy-Item -LiteralPath $Binary -Destination $StagedBinary -Force
    $PreviousBinary = Join-Path $Temporary "previous-binary.exe"
    if (Test-Path -LiteralPath $BinaryPath -PathType Leaf) { Copy-Item -LiteralPath $BinaryPath -Destination $PreviousBinary }
    Move-Item -LiteralPath $StagedBinary -Destination $BinaryPath -Force
    try {
        Write-Receipt $ReceiptPath $Version $InstallDir $DataDir $BinaryPath $OwnedManuals $PathAdded
    } catch {
        if (Test-Path -LiteralPath $PreviousBinary -PathType Leaf) { Copy-Item -LiteralPath $PreviousBinary -Destination $BinaryPath -Force }
        else { Remove-Item -LiteralPath $BinaryPath -Force }
        throw
    }
    if (-not $NoModifyPath -and $Receipt -and $Receipt.pathAdded -and (Test-PathEntry $Receipt.installDir $LegacyInstallDir) -and -not (Test-PathEntry $Receipt.installDir $InstallDir)) {
        Remove-PathEntry $Receipt.installDir
    }
    if (-not $NoModifyPath) {
        try {
            if (Add-UserPath $InstallDir) {
                $PathAdded = $true
                Write-Receipt $ReceiptPath $Version $InstallDir $DataDir $BinaryPath $OwnedManuals $PathAdded
            }
            Add-ProcessPath $InstallDir
        } catch {
            Write-Warning "could not update PATH: $($_.Exception.Message)"
        }
    }

    if (-not $CurrentVersion) {
        $Action = "Installed"
    } elseif ($CurrentVersion -eq $Version) {
        $Action = "Reinstalled"
    } else {
        $Action = "Updated"
    }
    $Message = "$Action ManT $Version"
    if ($Action -eq "Updated") {
        $Message += " (from $CurrentVersion)"
    }
    Write-Host $Message
    Write-Host "  executable: $BinaryPath"
    if ($OwnedManuals.Count) {
        Write-Host "  manuals:    $DataDir"
    }
    if ($NoModifyPath) {
        Write-Host ""
        Write-Host "Add $InstallDir to PATH, then run: mant mant"
    } else {
        Write-Host ""
        Write-Host "Run: mant mant"
    }
} finally {
    Remove-Item $Temporary -Recurse -Force -ErrorAction SilentlyContinue
}
}
