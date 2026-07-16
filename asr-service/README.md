# EchoNote ASR Service

This directory contains the local Python ASR service skeleton for EchoNote.

M2 provides a runnable FastAPI service with fake local transcription. The schema
contracts in `echonote_asr/schemas.py` remain the public API shape for the
plugin-facing integration.

M3 adds an optional `mlx-audio` backend and a standalone real-ASR spike command.
The default service backend remains `fake` until the real model path is fully
validated.

## Install

From this directory:

```bash
python -m venv .venv
. .venv/bin/activate
pip install -e .
```

Optional real MLX ASR dependencies:

```bash
pip install -e '.[mlx]'
```

## Run

```bash
python -m echonote_asr --host 127.0.0.1 --port 8765 --model mlx-community/Qwen3-ASR-0.6B-4bit --log-level info
```

The installed console script is also available:

```bash
echonote-asr --host 127.0.0.1 --port 8765 --model mlx-community/Qwen3-ASR-0.6B-4bit --log-level info
```

CLI options:

- `--host`: bind host, default `127.0.0.1`
- `--port`: bind port, default `8765`
- `--model`: initial model identifier, default `mlx-community/Qwen3-ASR-0.6B-4bit`
- `--backend`: `fake` or `mlx-audio`, default `fake`
- `--log-level`: one of `critical`, `error`, `warning`, `info`, `debug`

Logs are emitted as JSON lines.

## ASR Scheduling And Timings

Model inference is serialized and uses one bounded temporary workspace per service process. Each input WAV is removed
after inference, including failed requests, and the workspace is removed on service shutdown. JSON logs include
`lock_wait_ms`, `temp_write_ms`, `inference_ms`, `cleanup_ms`, and `response_serialize_ms` for transcription.
Finalization logs include diarization queue wait, model load, inference, assignment, merge, and cleanup durations.

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
  --model mlx-community/Qwen3-ASR-0.6B-4bit \
  --language zh
```

The command prints JSON with Python version, platform, model ID, load time,
transcription time, and transcript text.

To run the HTTP service with the real backend:

```bash
python -m echonote_asr \
  --host 127.0.0.1 \
  --port 8765 \
  --model mlx-community/Qwen3-ASR-0.6B-4bit \
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
  -d '{"model_id":"mlx-community/Qwen3-ASR-0.6B-4bit"}'
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
