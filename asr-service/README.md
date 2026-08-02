# EchoNote ASR Service

This directory contains the local Python ASR service skeleton for EchoNote.

M2 provides a runnable FastAPI service with fake local transcription. The schema
contracts in `echonote_asr/schemas.py` remain the public API shape for the
plugin-facing integration.

The real backend loads ASR and diarization weights only from local directories.
Companion v0.9.0 installs its dependencies and models from a verified offline bundle.

## Offline Install

Use EchoNote Companion and the repository-level offline bundle workflow. Companion installs with `pip --no-index`; it never upgrades pip or falls back to a package index. Python 3.11+ must already be present.

For development only, an online source environment can still be prepared from this directory:

```bash
python -m venv .venv
. .venv/bin/activate
pip install -e .
```

Optional real MLX ASR dependencies:

```bash
pip install -e '.[mlx]'
```

Optional Windows CPU ASR dependencies:

```powershell
.\.venv\Scripts\python.exe -m pip install -e ".[windows-cpu]"
```

## Windows Server 2016 CPU Service

The Windows CPU target uses CPython 3.11.9, `faster-whisper`, and CTranslate2 INT8. It does not load CUDA or use a
GPU. The supported launcher always binds one Uvicorn worker to `127.0.0.1`; attempts to pass another host to the
CLI are rejected.

For the packaged offline ZIP, extract it to a local NTFS directory and run these commands from Windows PowerShell
5.1. The package already contains the model and all Windows x64 wheels:

```powershell
Set-Location C:\EchoNote\EchoNote-ASR-Windows-CPU-0.9.0-py311
.\verify-windows-cpu.ps1
.\install-windows-cpu.ps1 -PythonPath C:\Python311\python.exe
.\run-windows-cpu.ps1
```

The installer accepts any Python 3.11.x patch release and rejects other Python minor versions. No package index or
model registry is contacted during verification, installation, or startup.

Create the environment from a PowerShell prompt on the server:

```powershell
Set-Location C:\EchoNote\asr-service
C:\Python311\python.exe -m venv .venv
.\.venv\Scripts\python.exe -m pip install -e ".[windows-cpu]"
```

For a disconnected server, place the complete `cp311-win_amd64` wheelhouse on the machine and install without an
index. The wheelhouse must include the build dependencies as well as every transitive runtime dependency:

```powershell
.\.venv\Scripts\python.exe -m pip install `
  --no-index `
  --find-links C:\EchoNote\wheelhouse `
  --no-build-isolation `
  -e ".[windows-cpu]"
```

The model path must be a local faster-whisper/CTranslate2 model directory containing at least `config.json` and
`model.bin`. Start the headless service with:

```powershell
.\run-windows-cpu.ps1 `
  -ModelPath C:\EchoNote\models\faster-whisper-small `
  -Port 8765 `
  -CpuThreads 0
```

`CpuThreads=0` lets CTranslate2 select its runtime default. Use a positive value after benchmarking the target CPU.
The process emits JSON logs to standard output and can be stopped with `Ctrl+C` or `POST /shutdown`. It does not
require EchoNote Companion, Obsidian, WebView2, or an interactive audio device.

## Run Offline

```bash
HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 python -m echonote_asr --host 127.0.0.1 --port 8765 --model /absolute/path/to/Qwen3-ASR-0.6B-4bit --backend mlx-audio --log-level info
```

The installed console script is also available:

```bash
echonote-asr --host 127.0.0.1 --port 8765 --model /absolute/path/to/Qwen3-ASR-0.6B-4bit --backend mlx-audio --log-level info
```

CLI options:

- `--host`: compatibility option fixed to `127.0.0.1`; all other addresses are rejected
- `--port`: bind port, default `8765`
- `--model`: absolute local model directory; remote model identifiers are rejected by the real backend
- `--backend`: `fake`, `mlx-audio`, or `faster-whisper`, default `fake`
- `--cpu-threads`: CPU thread budget for `faster-whisper`; `0` uses the runtime default
- `--log-level`: one of `critical`, `error`, `warning`, `info`, `debug`

Logs are emitted as JSON lines.

## ASR Scheduling And Timings

Model inference is serialized and uses one bounded temporary workspace per service process. Each input WAV is removed
after inference, including failed requests, and the workspace is removed on service shutdown. JSON logs include
`lock_wait_ms`, `temp_write_ms`, `inference_ms`, `cleanup_ms`, and `response_serialize_ms` for transcription.
Finalization logs include diarization queue wait, model load, inference, assignment, merge, and cleanup durations.

The real `mlx-audio` and `faster-whisper` backends start loading the configured model in the background as soon as
the service starts.
Before the model reports `ready`, EchoNote runs a bounded one-second warm-up inference so the first meeting chunk does
not pay the cold-generation cost. MLX maps API language values to the Qwen3 prompt names (`zh` -> `Chinese`, `en` ->
`English`); faster-whisper uses the ISO values `zh` and `en`. `auto` keeps model-side language detection enabled.
Model load logs include `load_ms`, `warmup_ms`, and `warmed_up`.

Run `.venv/bin/python benchmarks/performance_benchmark.py` for the ASR and speaker-assignment benchmarks.

## Speaker Diarization Performance

Speaker diarization is serialized so retries cannot run multiple pyannote pipelines at the same time. On CPU,
EchoNote uses a balanced 20% segmentation step (80% overlap) and a two-thread compute budget by default. CUDA
and MPS keep pyannote's original 10% segmentation step unless explicitly overridden. The current community-1
pipeline requires `pyannote.audio` 4.x.

The defaults can be tuned with environment variables:

- `ECHONOTE_DIARIZATION_DEVICE`: `auto`, `cpu`, `cuda`, or `mps`.
- `ECHONOTE_DIARIZATION_CPU_THREADS`: positive integer; defaults to at most `2`.
- `ECHONOTE_DIARIZATION_SEGMENTATION_STEP`: value from `0.05` to `1.0`. Use `0.1` for pyannote's original
  maximum-accuracy setting; larger values reduce overlapping inference windows and CPU work but may miss short
  speaker changes.

For example, the lowest-impact CPU profile is:

```bash
ECHONOTE_DIARIZATION_DEVICE=cpu \
ECHONOTE_DIARIZATION_CPU_THREADS=1 \
ECHONOTE_DIARIZATION_SEGMENTATION_STEP=0.2 \
python -m echonote_asr --backend mlx-audio
```

## Real ASR Spike

Use a 16kHz mono PCM16 WAV file:

```bash
python -m echonote_asr.spike_real_asr \
  --audio /tmp/echonote-test.wav \
  --model /absolute/path/to/Qwen3-ASR-0.6B-4bit \
  --language zh
```

The command prints JSON with Python version, platform, model ID, load time,
transcription time, and transcript text.

To run the HTTP service with the real backend:

```bash
python -m echonote_asr \
  --host 127.0.0.1 \
  --port 8765 \
  --model /absolute/path/to/Qwen3-ASR-0.6B-4bit \
  --backend mlx-audio \
  --log-level info
```

## API Checks

Health:

```bash
curl http://127.0.0.1:8765/health
```

Model status:

```bash
curl http://127.0.0.1:8765/model/status
```

Load a specific model:

```bash
curl -X POST http://127.0.0.1:8765/model/load \
  -H 'Content-Type: application/json' \
  -d '{"model_id":"/absolute/path/to/Qwen3-ASR-0.6B-4bit"}'
```

Create a tiny WAV file for local testing:

```bash
python - <<'PY'
import math
import wave

sample_rate = 16000
with wave.open('/tmp/echonote-test.wav', 'wb') as f:
    f.setnchannels(1)
    f.setsampwidth(2)
    f.setframerate(sample_rate)
    frames = bytearray()
    for i in range(sample_rate // 4):
        value = int(12000 * math.sin(2 * math.pi * 440 * i / sample_rate))
        frames.extend(value.to_bytes(2, 'little', signed=True))
    f.writeframes(bytes(frames))
PY
```

Fake transcribe:

```bash
curl -X POST http://127.0.0.1:8765/transcribe \
  -F audio=@/tmp/echonote-test.wav \
  -F chunk_id=chunk-001 \
  -F started_at_ms=0 \
  -F ended_at_ms=250 \
  -F language=zh
```

The response is a `TranscriptSegment` and the fake text includes `chunk_id`.

Shutdown:

```bash
curl -X POST http://127.0.0.1:8765/shutdown
```
