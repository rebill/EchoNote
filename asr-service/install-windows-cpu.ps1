[CmdletBinding()]
param(
    [string]$PythonPath = "python",
    [string]$InstallRoot = $PSScriptRoot
)

$ErrorActionPreference = "Stop"
$Wheelhouse = Join-Path $InstallRoot "wheelhouse"
$Requirements = Join-Path $InstallRoot "requirements-windows-cpu.txt"
$VirtualEnvironment = Join-Path $InstallRoot ".venv"
$VirtualPython = Join-Path $VirtualEnvironment "Scripts\python.exe"

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Executable,

        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $Executable $Arguments"
    }
}

if (-not (Test-Path -LiteralPath $Wheelhouse -PathType Container)) {
    throw "Wheelhouse directory was not found: $Wheelhouse"
}
if (-not (Test-Path -LiteralPath $Requirements -PathType Leaf)) {
    throw "Locked requirements file was not found: $Requirements"
}

$PythonVersion = & $PythonPath -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}')"
if ($LASTEXITCODE -ne 0) {
    throw "Could not run Python: $PythonPath"
}
if (-not $PythonVersion.StartsWith("3.11.")) {
    throw "EchoNote Windows CPU requires Python 3.11.x; found $PythonVersion"
}

if (-not (Test-Path -LiteralPath $VirtualPython -PathType Leaf)) {
    Invoke-Checked -Executable $PythonPath -Arguments @("-m", "venv", $VirtualEnvironment)
}

Invoke-Checked -Executable $VirtualPython -Arguments @(
    "-m", "pip", "install",
    "--no-index",
    "--find-links", $Wheelhouse,
    "--require-hashes",
    "-r", $Requirements
)

$ServiceWheels = @(Get-ChildItem -LiteralPath $Wheelhouse -Filter "echonote_asr-*.whl" -File)
if ($ServiceWheels.Count -ne 1) {
    throw "Expected exactly one EchoNote ASR wheel; found $($ServiceWheels.Count)"
}
Invoke-Checked -Executable $VirtualPython -Arguments @(
    "-m", "pip", "install",
    "--no-index",
    "--no-deps",
    "--force-reinstall",
    $ServiceWheels[0].FullName
)
Invoke-Checked -Executable $VirtualPython -Arguments @(
    "-c",
    "import ctranslate2, echonote_asr, faster_whisper; print('EchoNote Windows CPU runtime is ready')"
)

Write-Host "Installed EchoNote ASR into $VirtualEnvironment"
Write-Host "Start it with: .\run-windows-cpu.ps1"
