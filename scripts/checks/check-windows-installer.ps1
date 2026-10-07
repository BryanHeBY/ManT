# Exercise the actual installer with local release assets and isolated storage.
# No network requests, real installations or persistent PATH writes are allowed.
param([Parameter(Mandatory = $true)][string]$Binary)

$ErrorActionPreference = "Stop"
$Installer = Join-Path (Split-Path -Parent $PSScriptRoot) "install.ps1"
$TestRoot = Join-Path ([IO.Path]::GetTempPath()) "mant-installer-lifecycle-$([guid]::NewGuid().ToString('N'))"
$Names = @("mant.md", "mant-ir.md", "mant-markdown.md", "mant-protocol.md", "mant-roff.md")
$EnvironmentNames = @("HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "MANT_CONFIG_HOME", "MANT_DATA_HOME", "MANT_CACHE_HOME", "MANT_INSTALL_DIR", "MANT_DATA_DIR", "MANT_VERSION", "Path")
$PreviousEnvironment = @{}
foreach ($Name in $EnvironmentNames) {
    $PreviousEnvironment[$Name] = [Environment]::GetEnvironmentVariable($Name, "Process")
}
$DownloadState = @{ Count = 0 }

function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Get-UnexpandedUserPath {
    # Environment.GetEnvironmentVariable(User) expands REG_EXPAND_SZ against
    # the current process. Our isolated USERPROFILE/APPDATA values change that
    # presentation without changing the persisted PATH. Compare raw storage.
    $Key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $false)
    if ($null -eq $Key) { return $null }
    try {
        return $Key.GetValue("Path", $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    } finally { $Key.Dispose() }
}
function Directory([string]$Path) { [IO.Directory]::CreateDirectory($Path) | Out-Null }
function Write-Text([string]$Path, [string]$Text) {
    Directory (Split-Path -Parent $Path)
    [IO.File]::WriteAllText($Path, $Text)
}
function Invoke-WebRequest([switch]$UseBasicParsing, [string]$Uri, [string]$OutFile) {
    $DownloadState.Count++
    $Asset = Split-Path -Leaf ([uri]$Uri).AbsolutePath
    Assert ($Asset -in @($ArchiveName, "SHA256SUMS")) "unexpected installer download: $Uri"
    Copy-Item -LiteralPath (Join-Path $Assets $Asset) -Destination $OutFile
}
function Invoke-RestMethod { throw "unexpected installer network request" }
function gh { Set-Variable -Name LASTEXITCODE -Value 1 -Scope 1 } # No authenticated remote calls.
function Select-Home([string]$Name) {
    foreach ($Variable in $EnvironmentNames | Where-Object { $_ -ne "Path" }) {
        [Environment]::SetEnvironmentVariable($Variable, $null, "Process")
    }
    $env:HOME = Join-Path $TestRoot $Name
    $env:USERPROFILE = $env:HOME
    $env:APPDATA = Join-Path $env:HOME "roaming"
    $env:LOCALAPPDATA = Join-Path $env:HOME "local"
    Directory $env:HOME
}
function Receipt-Path { Join-Path $env:HOME ".local/state/mant/install-receipt.json" }
function Read-Receipt { Get-Content -LiteralPath (Receipt-Path) -Raw | ConvertFrom-Json }
function Install([string]$ExecutableDirectory, [string]$DocumentDirectory, [switch]$NoManual, [switch]$Force) {
    & $Installer -Version $Version -InstallDir $ExecutableDirectory -DataDir $DocumentDirectory -NoManual:$NoManual -Force:$Force -NoModifyPath
}
function Uninstall { & $Installer -Uninstall -NoModifyPath }

try {
    $PreviousUserPath = Get-UnexpandedUserPath
    Assert (Test-Path -LiteralPath $Binary -PathType Leaf) "missing installer-test binary: $Binary"
    $VersionOutput = & $Binary --version
    Assert ($LASTEXITCODE -eq 0 -and $VersionOutput -match '^mant\s+(\S+)') "cannot identify installer-test binary"
    $Version = $Matches[1]
    $ArchiveName = "mant-$Version-windows-x64.zip"
    $Assets = Join-Path $TestRoot "assets"
    $Package = Join-Path $Assets "mant-$Version-windows-x64"
    Directory (Join-Path $Package "manuals")
    Copy-Item -LiteralPath $Binary -Destination (Join-Path $Package "mant.exe")
    foreach ($Name in $Names) { Write-Text (Join-Path $Package "manuals/$Name") "# Bundled $Name`n" }
    Write-Text (Join-Path $Package "manuals/manifest.txt") ($Names -join "`n")
    Compress-Archive -LiteralPath $Package -DestinationPath (Join-Path $Assets $ArchiveName)
    $Hash = (Get-FileHash -LiteralPath (Join-Path $Assets $ArchiveName) -Algorithm SHA256).Hash
    Write-Text (Join-Path $Assets "SHA256SUMS") "$Hash  $ArchiveName`n"

    # Execute only the real pure path helpers; do not invoke user PATH writers.
    $Tokens = $null; $Errors = $null
    $Ast = [Management.Automation.Language.Parser]::ParseFile($Installer, [ref]$Tokens, [ref]$Errors)
    Assert ($Errors.Count -eq 0) "installer syntax errors"
    foreach ($FunctionName in @("Test-AbsolutePath", "Normalize-PathEntry", "Test-PathEntry")) {
        $Definition = $Ast.Find({ param($Node) $Node -is [Management.Automation.Language.FunctionDefinitionAst] -and $Node.Name -eq $FunctionName }, $true)
        Assert ($null -ne $Definition) "missing path helper $FunctionName"
        . ([scriptblock]::Create($Definition.Extent.Text))
    }
    Assert (Test-PathEntry 'C:\Users\test\.local\bin' 'c:/Users/test/.local/./x/../bin/') "path separators, case or dot components differ"
    Assert (Test-PathEntry '\\server\share\docs' '//server/share/./docs/') "UNC paths differ"
    Assert (-not (Test-PathEntry 'C:\Users\test\.local\bin' 'C:\Users\test\.local\other')) "distinct directories compare equal"

    Select-Home "no-manual-first"
    $Bin = Join-Path $env:HOME "bin"
    $Docs = Join-Path $env:HOME "documents"
    Install $Bin $Docs -NoManual
    Assert (@((Read-Receipt).manuals).Count -eq 0) "initial no-manual receipt contains phantom ownership"
    Uninstall
    Assert (-not (Test-Path -LiteralPath (Join-Path $Bin "mant.exe"))) "initial no-manual installation could not uninstall"

    Select-Home "normal"
    $Bin = Join-Path $env:HOME "bin"
    $Docs = Join-Path $env:HOME "documents"
    Install $Bin $Docs
    Assert ((Read-Receipt).manuals.Count -eq 5) "fresh install did not register bundled manuals"
    $Before = $DownloadState.Count
    Install $Bin $Docs -NoManual
    Assert ($DownloadState.Count -eq $Before) "same-destination current version downloaded again"
    Assert ((Read-Receipt).manuals.Count -eq 5) "same-destination no-manual lost proven ownership"
    $NewDocs = Join-Path $env:HOME "user-documents"
    Write-Text (Join-Path $NewDocs "mant.md") "user-authored manual"
    Install $Bin $NewDocs -NoManual
    Assert (@((Read-Receipt).manuals).Count -eq 0) "no-manual claimed a user file at a new destination"
    Uninstall
    Assert ([IO.File]::ReadAllText((Join-Path $NewDocs "mant.md")) -eq "user-authored manual") "uninstall deleted user manual"

    Install $Bin $Docs
    Install $Bin $NewDocs -NoManual -Force
    Assert (@((Read-Receipt).manuals).Count -eq 0) "forced no-manual reinstall claimed a user file"
    Uninstall
    Assert ([IO.File]::ReadAllText((Join-Path $NewDocs "mant.md")) -eq "user-authored manual") "forced reinstall/uninstall deleted user manual"
    Install $Bin $Docs
    $NewBin = Join-Path $env:HOME "other-bin"
    $Before = $DownloadState.Count
    Install $NewBin $Docs -NoManual
    Assert ($DownloadState.Count -eq $Before + 2) "same-version relocation incorrectly took the fast path"
    Assert (Test-Path -LiteralPath (Join-Path $NewBin "mant.exe") -PathType Leaf) "relocation wrote a receipt without a binary"
    Assert ((Read-Receipt).binary -eq (Join-Path $NewBin "mant.exe")) "relocation receipt is incorrect"
    Uninstall
    Assert (-not (Test-Path -LiteralPath (Join-Path $NewBin "mant.exe"))) "relocated binary survived uninstall"

    Select-Home "home [case]"
    $Bin = Join-Path $env:HOME "bin [case]"
    $Docs = Join-Path $env:HOME "documents [case]"
    Install $Bin $Docs
    Write-Text (Join-Path $Docs "personal.md") "personal document"
    Uninstall
    Assert (-not (Test-Path -LiteralPath (Join-Path $Bin "mant.exe"))) "literal-path uninstall retained binary"
    foreach ($Name in $Names) { Assert (-not (Test-Path -LiteralPath (Join-Path $Docs $Name))) "literal-path uninstall retained $Name" }
    Assert (-not (Test-Path -LiteralPath (Receipt-Path))) "literal-path uninstall retained receipt"
    Assert (Test-Path -LiteralPath (Join-Path $Docs "personal.md")) "literal-path uninstall deleted user data"

    Select-Home "legacy"
    $LegacyRoot = Join-Path $env:APPDATA "ManT"
    $LegacyBin = Join-Path $env:LOCALAPPDATA "Programs/ManT/bin"
    Directory $LegacyBin
    Copy-Item -LiteralPath $Binary -Destination (Join-Path $LegacyBin "mant.exe")
    Write-Text (Join-Path $LegacyRoot "sources.toml") "# legacy configuration`n"
    Write-Text (Join-Path $LegacyRoot "documents/mant.md") "# old bundled manual`n"
    Write-Text (Join-Path $LegacyRoot "documents/personal.md") "# personal`n"
    $LegacyReceipt = Join-Path $env:LOCALAPPDATA "ManT/install-receipt.json"
    Write-Text $LegacyReceipt ([ordered]@{
        schema = "mant.install/v1"; version = "0.11.0"; layout = "legacy"
        installDir = ($LegacyBin.Replace('\', '/') + '/../bin/')
        dataDir = (Join-Path $LegacyRoot "documents")
        binary = (Join-Path $LegacyBin "mant.exe")
        manuals = @((Join-Path $LegacyRoot "documents/mant.md")); pathAdded = $false
    } | ConvertTo-Json)
    & $Installer -Version $Version -NoManual -NoModifyPath
    $DefaultBin = Join-Path $env:HOME ".local/bin/mant.exe"
    $DefaultDocs = Join-Path $env:HOME ".local/share/mant/documents"
    Assert (Test-Path -LiteralPath $DefaultBin) "normalized legacy default did not migrate binary"
    Assert (Test-Path -LiteralPath (Join-Path $env:HOME ".config/mant/sources.toml")) "legacy configuration did not migrate"
    Assert ((Read-Receipt).layout -eq "unix-v1") "migration receipt lacks completed layout"
    Assert (@((Read-Receipt).manuals).Count -eq 0) "no-manual migration inferred ownership without a copy receipt"
    Uninstall
    Assert (Test-Path -LiteralPath (Join-Path $DefaultDocs "mant.md")) "no-manual uninstall deleted migrated unclaimed manual"
    Assert (Test-Path -LiteralPath (Join-Path $DefaultDocs "personal.md")) "migration uninstall deleted personal document"
    Assert (Test-Path -LiteralPath (Join-Path $LegacyRoot "documents/mant.md")) "migration deleted original manual"

    Write-Text (Join-Path $DefaultDocs "personal.md") "# conflicting destination`n"
    $Failed = $false
    try { & $Installer -Version $Version -NoManual -NoModifyPath }
    catch { $Failed = $true }
    Assert $Failed "conflicting legacy data did not stop installation"
    Assert (-not (Test-Path -LiteralPath $DefaultBin)) "conflicting migration activated a binary"
    Assert (-not (Test-Path -LiteralPath (Receipt-Path))) "conflicting migration published a receipt"
    Assert (Test-Path -LiteralPath (Join-Path $LegacyBin "mant.exe")) "conflicting migration removed old binary"
    Assert ([IO.File]::ReadAllText((Join-Path $DefaultDocs "personal.md")) -eq "# conflicting destination`n") "conflicting migration overwrote destination data"

    Assert ((Get-UnexpandedUserPath) -ceq $PreviousUserPath) "installer suite changed persistent user PATH"
    Write-Host "Windows installer lifecycle checks passed"
} finally {
    foreach ($Name in $EnvironmentNames) {
        [Environment]::SetEnvironmentVariable($Name, $PreviousEnvironment[$Name], "Process")
    }
    Remove-Item -LiteralPath $TestRoot -Recurse -Force -ErrorAction SilentlyContinue
}
