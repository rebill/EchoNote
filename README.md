# EchoNote

EchoNote is an Obsidian desktop plugin for local meeting transcription and AI meeting summaries.

MVP scope:

- macOS only.
- Local ASR with MLX.
- Default ASR model: `mlx-community/Qwen3-ASR-0.6B-4bit`.
- Quasi-real-time chunk transcription.
- Markdown meeting notes in your Obsidian vault.
- OpenAI-compatible and Anthropic summary providers.
- Optional virtual audio input support through BlackHole or Loopback.

## Repository Layout

```text
plugin/       Obsidian TypeScript plugin
asr-service/  Local Python ASR service
docs/         PRD, technical design, test plans, and user guides
```

## Requirements

- macOS.
- Obsidian Desktop.
- Node.js for building the plugin.
- Python 3.11+.
- Apple Silicon Mac recommended for MLX.
- Optional: BlackHole or Loopback if you want to record meeting software output.

## Install ASR Service

From the repository root:

```bash
cd /Users/br/Git/github/rebill/EchoNote

python3 -m venv asr-service/.venv
source asr-service/.venv/bin/activate

pip install --upgrade pip
pip install -e 'asr-service[mlx]'
```

Verify real ASR:

```bash
python -m echonote_asr.spike_real_asr \
  --audio /tmp/echonote-test.wav \
  --model mlx-community/Qwen3-ASR-0.6B-4bit \
  --language zh
```

## Run ASR Service

For fake ASR testing:

```bash
source asr-service/.venv/bin/activate

python -m echonote_asr \
  --host 127.0.0.1 \
  --port 8765 \
  --model mlx-community/Qwen3-ASR-0.6B-4bit \
  --backend fake \
  --log-level info
```

For real local ASR:

```bash
source asr-service/.venv/bin/activate

python -m echonote_asr \
  --host 127.0.0.1 \
  --port 8765 \
  --model mlx-community/Qwen3-ASR-0.6B-4bit \
  --backend mlx-audio \
  --log-level info
```

Check service health:

```bash
curl http://127.0.0.1:8765/health
```

## Build Obsidian Plugin

```bash
cd plugin
npm install
npm run build
```

The build creates:

```text
plugin/main.js
plugin/manifest.json
```

## Install Plugin Into A Vault

Replace the vault path with your test vault path:

```bash
mkdir -p "/path/to/TestVault/.obsidian/plugins/echonote"

cp /Users/br/Git/github/rebill/EchoNote/plugin/main.js \
   /Users/br/Git/github/rebill/EchoNote/plugin/manifest.json \
   "/path/to/TestVault/.obsidian/plugins/echonote/"
```

Then in Obsidian:

```text
Settings → Community plugins → EchoNote → Enable
```

## EchoNote Settings

Recommended MVP settings:

```text
Python path:
/Users/br/Git/github/rebill/EchoNote/asr-service/.venv/bin/python

ASR service path:
/Users/br/Git/github/rebill/EchoNote/asr-service

ASR service port:
8765

ASR model:
mlx-community/Qwen3-ASR-0.6B-4bit

Audio input device:
Default audio input / BlackHole / Loopback device
```

For AI summaries, configure either:

- OpenAI-compatible API key, base URL, and model.
- Anthropic API key and model.

## Basic Workflow

1. Start the ASR service.
2. Open Obsidian and enable EchoNote.
3. Open `EchoNote Status`.
4. Select an audio input device in EchoNote settings.
5. Click `Start Meeting`.
6. Speak or play meeting audio.
7. Click `Stop Meeting`.
8. Open the generated meeting note under `Meetings/`.
9. Click `Summarize Meeting`.

## Recording Meeting Software Audio

EchoNote records from an audio input device. To record Zoom, Google Meet, Feishu, or other meeting software output, configure a virtual audio route.

See:

- [macOS Virtual Audio Guide](docs/MAC_VIRTUAL_AUDIO_GUIDE.md)

Typical setup:

```text
System Output: Multi-Output Device
EchoNote Input: BlackHole 2ch
```

For a mixed source containing both your microphone and meeting software output, Loopback is the easier option.

## Documentation

- [Product Requirements](docs/PRD.md)
- [Technical Design](docs/TECH_DESIGN.md)
- [Delivery Plan](docs/DELIVERY_PLAN.md)
- [API Contract](docs/API_CONTRACT.md)
- [M3 Real ASR Spike](docs/M3_REAL_ASR_SPIKE.md)
- [M6 E2E Test Plan](docs/M6_E2E_TEST_PLAN.md)
- [macOS Virtual Audio Guide](docs/MAC_VIRTUAL_AUDIO_GUIDE.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md)
- [Privacy](docs/PRIVACY.md)

## Privacy

- ASR transcription runs locally.
- Meeting audio is not sent to cloud ASR providers.
- Raw audio is not saved by default.
- If AI summary uses a cloud LLM provider, the transcript is sent to that provider.

See [Privacy](docs/PRIVACY.md).
