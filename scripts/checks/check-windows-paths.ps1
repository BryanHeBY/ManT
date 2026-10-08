# Run the real installer's pure path helpers against the shared Rust fixtures.
# This check works on any PowerShell host and never runs an installation or PATH writer.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Tokens = $null
$Errors = $null
$Ast = [Management.Automation.Language.Parser]::ParseFile(
    (Join-Path $Root "scripts/install.ps1"), [ref]$Tokens, [ref]$Errors
)
if ($Errors.Count) { throw ($Errors | Out-String) }
foreach ($FunctionName in @("Get-AbsolutePathRoot", "Test-AbsolutePath", "Normalize-PathEntry", "Test-PathEntry")) {
    $Definition = $Ast.Find({ param($Node) $Node -is [Management.Automation.Language.FunctionDefinitionAst] -and $Node.Name -eq $FunctionName }, $true)
    if ($null -eq $Definition) { throw "missing installer path helper $FunctionName" }
    . ([scriptblock]::Create($Definition.Extent.Text))
}
$Fixture = Join-Path $Root "crates/mant-sources/src/settings/windows_paths.tsv"
foreach ($Line in Get-Content -LiteralPath $Fixture -Encoding UTF8) {
    if ($Line.StartsWith('#')) { continue }
    $Fields = $Line.Split([char]9)
    $Expected = [bool]::Parse($Fields[1])
    switch ($Fields[0]) {
        "absolute" { $Actual = Test-AbsolutePath $Fields[2] }
        "equal" { $Actual = Test-PathEntry $Fields[2] $Fields[3] }
        default { throw "unknown path fixture: $Line" }
    }
    if ($Actual -ne $Expected) { throw "installer path contract mismatch: $Line (actual: $Actual)" }
}
foreach ($Code in @(0, 10, 31, 127, 133)) {
    $Path = "C:\bad$([char]$Code)path"
    if ((Test-AbsolutePath $Path) -or (Test-PathEntry $Path $Path)) { throw "accepted path control U+$Code" }
}
if (Test-AbsolutePath ('C:\' + ('x' * 4094))) { throw "accepted an oversized installer path" }
Write-Host "Shared Windows path contract passed."
