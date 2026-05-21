# EchoNote

EchoNote is an Obsidian desktop plugin for local meeting transcription and AI meeting summaries.

MVP scope:

- macOS only.
- Local ASR with MLX.
- Optional macOS ASR Companion in v0.2.0 to start, stop, monitor, and diagnose the local ASR service.
- Quasi-real-time chunk transcription.
- Markdown meeting notes in your Obsidian vault.
- OpenAI-compatible and Anthropic summary providers.
- Optional virtual audio input support through BlackHole or Loopback.

## Repository Layout

```text
plugin/       Obsidian TypeScript plugin
asr-service/  Local Python ASR service
companion/    Tauri ASR Companion app, added by the v0.2.0 scaffold task
docs/         PRD, technical design, test plans, and user guides
```

## Requirements

- macOS.
- Obsidian Desktop.
- Node.js for building the plugin.
- Python 3.11+.
- Apple Silicon Mac recommended for MLX.
- Rust and Tauri prerequisites if building the v0.2.0 Companion from source.
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

## ASR Runtime

EchoNote v0.2.0 uses EchoNote ASR Companion as the only ASR backend for the Obsidian plugin. The plugin reads Companion discovery and calls the Companion-managed localhost ASR API; it no longer starts its own Python ASR process.

If discovery is missing, stale, invalid, or unhealthy, EchoNote shows an explicit Companion error.

## Run With Companion

The v0.2.0 Companion is a macOS Tauri app that manages the existing Python ASR service. It does not bundle Python or model weights in the MVP. v0.2.0 is source-only for Companion; no signed `.app` or `.dmg` is published for this release.

Expected workflow:

1. Install the ASR service environment from `Install ASR Service`.
2. Build and run Companion from source:

```bash
cd companion
npm install
npm run tauri:dev
```

3. Open EchoNote ASR Companion.
4. Configure:
   - Python executable path.
   - ASR service directory.
   - Port, usually `8765`.
   - Backend: `fake` for smoke tests or `mlx-audio` for local ASR.
   - Model ID.
5. Click `Start Service`.
6. Confirm the Companion shows `Service: Running` and writes discovery to:

```text
~/Library/Application Support/EchoNote/companion.json
```

Then in Obsidian, open EchoNote Status and confirm Companion status is `available`.

For the release smoke path, run:

```bash
node scripts/v0_2_0_fake_backend_smoke.mjs
```

This starts the fake backend, writes a temporary discovery file, verifies the plugin resolves Companion, and verifies legacy Manual settings migrate away from plugin-managed ASR.

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
plugin/styles.css
```

## Install Plugin Into A Vault

Replace the vault path with your test vault path:

```bash
mkdir -p "/path/to/TestVault/.obsidian/plugins/echonote"

cp /Users/br/Git/github/rebill/EchoNote/plugin/main.js \
   /Users/br/Git/github/rebill/EchoNote/plugin/manifest.json \
   /Users/br/Git/github/rebill/EchoNote/plugin/styles.css \
   "/path/to/TestVault/.obsidian/plugins/echonote/"
```

Then in Obsidian:

```text
Settings → Community plugins → EchoNote → Enable
```

## EchoNote Settings

Recommended MVP settings:

```text
Companion discovery path:
~/Library/Application Support/EchoNote/companion.json

Audio input device:
Default audio input / BlackHole / Loopback device
```

For AI summaries, configure either:

- OpenAI-compatible API key, base URL, and model.
- Anthropic API key and model.

## Basic Workflow

1. Open EchoNote ASR Companion and click `Start Service`.
2. Open Obsidian and enable EchoNote.
3. Open `EchoNote Status`.
4. Confirm Companion status is `available`.
5. Select an audio input device in EchoNote settings.
6. Click `Start Meeting`.
7. Speak or play meeting audio.
8. Click `Stop Meeting`.
9. Open the generated meeting note under `Meetings/`.
10. Click `Summarize Meeting`.

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

- [Changelog](CHANGELOG.md)
- [Contributing](CONTRIBUTING.md)
- [Product Requirements](docs/PRD.md)
- [v0.2.0 Tauri ASR Companion PRD](docs/V0_2_0_TAURI_COMPANION_PRD.md)
- [v0.2.0 Tauri ASR Companion Technical Design](docs/V0_2_0_TAURI_COMPANION_TECH_DESIGN.md)
- [v0.2.0 Tauri ASR Companion Tasks](docs/V0_2_0_TAURI_COMPANION_TASKS.md)
- [Technical Design](docs/TECH_DESIGN.md)
- [Delivery Plan](docs/DELIVERY_PLAN.md)
- [Release Process](docs/RELEASE.md)
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
