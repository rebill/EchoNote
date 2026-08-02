[CmdletBinding()]
param(
    [string]$BundleRoot = $PSScriptRoot
)

$ErrorActionPreference = "Stop"
$ChecksumsPath = Join-Path $BundleRoot "SHA256SUMS.txt"
if (-not (Test-Path -LiteralPath $ChecksumsPath -PathType Leaf)) {
    throw "Checksum manifest was not found: $ChecksumsPath"
}

$Verified = 0
foreach ($Line in Get-Content -LiteralPath $ChecksumsPath) {
    if (-not $Line.Trim()) {
        continue
    }
    if ($Line -notmatch "^([0-9a-f]{64})  (.+)$") {
        throw "Invalid checksum entry: $Line"
    }

    $Expected = $Matches[1]
    $RelativePath = $Matches[2].Replace("/", "\")
    $FilePath = Join-Path $BundleRoot $RelativePath
    if (-not (Test-Path -LiteralPath $FilePath -PathType Leaf)) {
        throw "Bundle file is missing: $RelativePath"
    }

    $Actual = (Get-FileHash -LiteralPath $FilePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) {
        throw "Checksum mismatch: $RelativePath"
    }
    $Verified += 1
}

Write-Host "Verified $Verified bundle files."
