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

function Fail([string]$Message) {
    throw "mant installer: $Message"
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
  -NoManual               Do not install the bundled mant.md manual
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
        "$($UserPath.TrimEnd(';'));$Directory"
    } else {
        $Directory
    }
    [Environment]::SetEnvironmentVariable("Path", $NewUserPath, "User")
    return $true
}

function Add-ProcessPath([string]$Directory) {
    $Entries = @($env:Path -split ';' | Where-Object { $_ })
    if (-not ($Entries | Where-Object { Test-PathEntry $_ $Directory })) {
        $env:Path = "$env:Path;$Directory"
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

function Validate-AbsolutePath([string]$Path, [string]$Label) {
    if (-not $Path -or -not [IO.Path]::IsPathRooted($Path)) {
        Fail "$Label must be an absolute path"
    }
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
    [string]$Manual,
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
        manual = $Manual
        pathAdded = $PathAdded
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
if (-not $env:LOCALAPPDATA) {
    Fail "LOCALAPPDATA is required"
}

# Windows PowerShell 5.1 can otherwise negotiate an obsolete TLS version.
if ($PSVersionTable.PSEdition -eq "Desktop") {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
}

$Repository = "BryanHeBY/ManT"
$GitHub = "https://github.com/$Repository"
$ReceiptPath = Join-Path $env:LOCALAPPDATA "ManT\install-receipt.json"
$Receipt = $null
if (Test-Path -PathType Leaf $ReceiptPath) {
    try {
        $Receipt = Get-Content $ReceiptPath -Raw | ConvertFrom-Json
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
    if ($Receipt.manual) {
        Validate-AbsolutePath $Receipt.manual "receipt manual path"
        $ExpectedManual = Join-Path $Receipt.dataDir "mant.md"
        if (-not (Test-PathEntry $Receipt.manual $ExpectedManual)) {
            Fail "installer receipt contains an invalid manual path"
        }
    }

    $Removed = $false
    if (Test-Path -PathType Leaf $Receipt.binary) {
        Remove-Item $Receipt.binary -Force
        Write-Host "Removed $($Receipt.binary)"
        $Removed = $true
    }
    if ($Receipt.manual -and (Test-Path -PathType Leaf $Receipt.manual)) {
        Remove-Item $Receipt.manual -Force
        Write-Host "Removed $($Receipt.manual)"
        $Removed = $true
    }
    if ($Receipt.pathAdded) {
        Remove-PathEntry $Receipt.installDir
        Write-Host "Removed $($Receipt.installDir) from user PATH"
    }
    Remove-Item $ReceiptPath -Force

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
    } elseif ($Receipt) {
        $Receipt.installDir
    } else {
        Join-Path $env:LOCALAPPDATA "Programs\ManT\bin"
    }
}
if (-not $DataDir) {
    $DataDir = if ($env:MANT_DATA_DIR) {
        $env:MANT_DATA_DIR
    } elseif ($Receipt) {
        $Receipt.dataDir
    } else {
        if (-not $env:APPDATA) {
            Fail "APPDATA is required"
        }
        Join-Path $env:APPDATA "ManT\documents"
    }
}
Validate-AbsolutePath $InstallDir "install directory"
Validate-AbsolutePath $DataDir "data directory"

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
$Target = "windows-x64"
$Archive = "mant-$Version-$Target.zip"
$ReleaseUrl = "$GitHub/releases/download/$Tag"
$BinaryPath = Join-Path $InstallDir "mant.exe"
$ManualPath = Join-Path $DataDir "mant.md"
$CurrentVersion = Get-InstalledVersion $BinaryPath
$OwnedManual = if (-not $NoManual) {
    $ManualPath
} elseif ($Receipt -and (Test-PathEntry $Receipt.dataDir $DataDir)) {
    [string]$Receipt.manual
} else {
    ""
}
$ManualReady = $NoManual -or (Test-Path -PathType Leaf $ManualPath)
$PathAdded = [bool]($Receipt -and $Receipt.pathAdded)

if (-not $Force -and $CurrentVersion -eq $Version -and $ManualReady) {
    Write-Receipt $ReceiptPath $Version $InstallDir $DataDir $BinaryPath $OwnedManual $PathAdded
    if (-not $NoModifyPath) {
        try {
            if (Add-UserPath $InstallDir) {
                $PathAdded = $true
                Write-Receipt $ReceiptPath $Version $InstallDir $DataDir $BinaryPath $OwnedManual $PathAdded
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

    $Expanded = Join-Path $Temporary "expanded"
    Expand-Archive -Path $ArchivePath -DestinationPath $Expanded
    $Package = Join-Path $Expanded "mant-$Version-$Target"
    $Binary = Join-Path $Package "mant.exe"
    $Manual = Join-Path $Package "mant.md"
    if (-not (Test-Path -PathType Leaf $Binary)) {
        Fail "$Archive does not contain mant.exe"
    }
    if (-not (Test-Path -PathType Leaf $Manual)) {
        Fail "$Archive does not contain the ManT manual"
    }

    New-Item $InstallDir -ItemType Directory -Force | Out-Null
    Copy-Item $Binary $BinaryPath -Force
    if (-not $NoManual) {
        New-Item $DataDir -ItemType Directory -Force | Out-Null
        Copy-Item $Manual $ManualPath -Force
    } elseif ($OwnedManual -and -not (Test-Path -PathType Leaf $OwnedManual)) {
        $OwnedManual = ""
    }
    Write-Receipt $ReceiptPath $Version $InstallDir $DataDir $BinaryPath $OwnedManual $PathAdded
    if (-not $NoModifyPath) {
        try {
            if (Add-UserPath $InstallDir) {
                $PathAdded = $true
                Write-Receipt $ReceiptPath $Version $InstallDir $DataDir $BinaryPath $OwnedManual $PathAdded
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
    if ($OwnedManual) {
        Write-Host "  manual:     $OwnedManual"
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
