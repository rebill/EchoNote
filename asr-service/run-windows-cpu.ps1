[CmdletBinding()]
param(
    [string]$ModelPath = (Join-Path $PSScriptRoot "models\faster-whisper-small"),

    [ValidateRange(1, 65535)]
    [int]$Port = 8765,

    [ValidateRange(0, 256)]
    [int]$CpuThreads = 0,

    [ValidateSet("critical", "error", "warning", "info", "debug")]
    [string]$LogLevel = "info",

    [string]$PythonPath = (Join-Path $PSScriptRoot ".venv\Scripts\python.exe")
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $PythonPath -PathType Leaf)) {
    throw "Python executable was not found: $PythonPath"
}
if (-not (Test-Path -LiteralPath $ModelPath -PathType Container)) {
    throw "Offline faster-whisper model directory was not found: $ModelPath"
}

$ResolvedPythonPath = (Resolve-Path -LiteralPath $PythonPath).Path
$ResolvedModelPath = (Resolve-Path -LiteralPath $ModelPath).Path
foreach ($RequiredFile in @("config.json", "model.bin")) {
    $RequiredPath = Join-Path $ResolvedModelPath $RequiredFile
    if (-not (Test-Path -LiteralPath $RequiredPath -PathType Leaf)) {
        throw "Offline faster-whisper model is missing ${RequiredFile}: $ResolvedModelPath"
    }
}

$env:HF_HUB_OFFLINE = "1"
$env:TRANSFORMERS_OFFLINE = "1"
$env:HF_DATASETS_OFFLINE = "1"
Remove-Item Env:HUGGINGFACE_HUB_TOKEN -ErrorAction SilentlyContinue
Remove-Item Env:HF_TOKEN -ErrorAction SilentlyContinue

$ExitCode = 0
Push-Location $PSScriptRoot
try {
    & $ResolvedPythonPath -m echonote_asr `
        --host 127.0.0.1 `
        --port $Port `
        --model $ResolvedModelPath `
        --backend faster-whisper `
        --cpu-threads $CpuThreads `
        --log-level $LogLevel
    $ExitCode = $LASTEXITCODE
}
finally {
    Pop-Location
}

exit $ExitCode
