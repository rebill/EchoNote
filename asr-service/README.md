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
