# M0 on Windows: build, smoke-test, capture probe and (optionally) the benchmark matrix.
# Prerequisites: rustup with the MSVC toolchain (Visual Studio Build Tools), Python 3.11+, uv.
#
#   powershell -ExecutionPolicy Bypass -File scripts\dev\windows_m0.ps1 -Step build
#   ... -Step capture [-Seconds 45]   records mic + loopback into results\capture-probe-*
#   ... -Step bench                   long run: only when the machine is idle
#
# Set SHERPA_ONNX_ARCHIVE_DIR to a folder holding
# sherpa-onnx-v2.0.1-win-x64-static-MT-Release-lib.tar.bz2 if GitHub downloads are slow.
param(
    [ValidateSet("build", "capture", "bench")] [string] $Step = "build",
    [int] $Seconds = 45
)
$ErrorActionPreference = "Stop"
$Repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Data = if ($env:ECHONOTE_DEV_DATA) { $env:ECHONOTE_DEV_DATA } else { Join-Path $HOME ".cache\echonote-dev" }
Set-Location $Repo

function Run([string] $Exe, [string[]] $Arguments) {
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Exe $($Arguments -join ' ') failed with $LASTEXITCODE" }
}

switch ($Step) {
    "build" {
        Run cargo @("build", "--release", "-p", "echonote-bench")
        Run cargo @("build", "--release", "-p", "echonote-core", "--example", "capture_probe")
        $bench = Join-Path $Repo "target\release\echonote-bench.exe"
        "echonote-bench.exe: {0:N1} MB" -f ((Get-Item $bench).Length / 1MB)
        $dumpbin = Get-Command dumpbin -ErrorAction SilentlyContinue
        if ($dumpbin) { & dumpbin /dependents $bench | Select-String "\.dll" }
        else { "dumpbin not on PATH; run from a Developer PowerShell to list DLL dependencies" }
        Run uv @("run", "scripts/dev/fetch_assets.py")
        Run uv @("run", "scripts/dev/build_eval_sets.py")
        $wav = Get-ChildItem (Join-Path $Data "eval\aishell1-test-s12\wav") -Filter *.wav | Select-Object -First 1
        Run $bench @("smoke", $wav.FullName)
    }
    "capture" {
        $out = Join-Path $Data ("results\capture-probe-" + (Get-Date -Format "yyyyMMdd-HHmmss"))
        $third = [int]($Seconds / 3)
        "Recording $Seconds s into $out"
        "  0-$third s: play a video with speech through the speakers, stay silent"
        "  $third-$(2 * $third) s: pause playback (silence on the output) and speak"
        "  $(2 * $third)-$Seconds s: resume playback and keep speaking"
        Run (Join-Path $Repo "target\release\examples\capture_probe.exe") @($out, "$Seconds")
        Run uv @("run", "scripts/dev/analyze_capture_probe.py", $out)
    }
    "bench" {
        Run python @("scripts/dev/run_m0_bench.py")
    }
}
