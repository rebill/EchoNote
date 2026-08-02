# EchoNote v0.9.0 Offline Installation

## Scope

EchoNote v0.9.0 guarantees offline runtime and offline installation for:

- Qwen3 ASR through MLX Audio.
- Local `pyannote.audio` speaker diarization.
- Companion setup, dependency repair, model verification, and service startup.

`Summarize Meeting` and LLM transcript correction are intentionally outside this guarantee. They still require the configured OpenAI-compatible or Anthropic provider unless a separate local LLM provider is configured in a future release.

Python 3.11+ must already exist on the target Mac. v0.9.0 does not bundle a Python interpreter.

## Bundle Contract

```text
offline-bundle/
  echonote-offline-manifest.json
  requirements-offline.txt
  wheels/
  models/
    Qwen3-ASR-0.6B-4bit/
    speaker-diarization-community-1/
```

The manifest records the schema, bundle version, target OS/architecture, Python major/minor, model roles and presets, and the size plus SHA-256 of every bundled file. Installation rejects:

- Wrong platform or Python minor version.
- Missing, extra, duplicate, modified, or symlinked files.
- Absolute paths and `..` traversal in the manifest.
- Missing ASR directories or diarization `config.yaml`.

## Build on an Authorized Connected Mac

First create a clean connected build environment using the same Python major/minor and architecture as the target. Install the ASR dependency set, freeze the resolved environment, and download every frozen package into a wheelhouse:

```bash
python3.11 -m venv /tmp/echonote-bundle-build
/tmp/echonote-bundle-build/bin/python -m pip install \
  'fastapi>=0.110.0' 'python-multipart>=0.0.9' 'uvicorn[standard]>=0.29.0' \
  'mlx-audio>=0.1.0' 'huggingface_hub>=0.23.0' 'pyannote.audio>=4.0.0,<5.0.0'
/tmp/echonote-bundle-build/bin/python -m pip freeze \
  > /tmp/requirements-offline.txt
/tmp/echonote-bundle-build/bin/python -m pip download \
  --only-binary=:all: \
  --dest /tmp/echonote-wheelhouse \
  -r /tmp/requirements-offline.txt
```

The requirements file must contain package/version requirements only; remote URLs, Git references, editable installs, and index overrides are rejected. If `pip download --only-binary` fails, the target package does not provide a compatible wheel and must be replaced or built into a wheel on the connected build machine before continuing.

```bash
python3 scripts/build_offline_bundle.py \
  --output /tmp/echonote-offline-bundle \
  --bundle-version 0.9.0 \
  --wheelhouse /path/to/wheelhouse \
  --requirements /path/to/requirements-offline.txt \
  --asr-model qwen3-0.6b-4bit=/path/to/Qwen3-ASR-0.6B-4bit \
  --asr-model qwen3-1.7b-4bit=/path/to/Qwen3-ASR-1.7B-4bit \
  --diarization-model /path/to/speaker-diarization-community-1
```

Hugging Face cache snapshots often contain symlinks into the blob store. The builder materializes those links as regular files so the resulting bundle is portable.

Model licenses and redistribution permissions remain the distributor's responsibility. The supported architecture is to export model weights that the user or organization is authorized to use.

## Install Without Network Access

On the build machine, create the side-loadable Obsidian plugin and Companion app:

```bash
cd plugin && npm run package
cd ../companion && npm run tauri:build
```

Transfer these artifacts together with the offline bundle:

- `dist/echonote/` — Obsidian plugin files.
- `companion/src-tauri/target/release/bundle/macos/EchoNote.app` — Companion with the ASR service source embedded.
- The generated `offline-bundle/` — wheels and authorized local model files.

On the disconnected Mac:

1. Copy `dist/echonote/` to `<Vault>/.obsidian/plugins/echonote/` and enable EchoNote in Community plugins.
2. Copy `EchoNote.app` to `/Applications` and open it.
3. Copy or mount the entire offline bundle directory.
4. In Companion Advanced Settings, set `Offline bundle path`. The default is `~/Library/Application Support/EchoNote/offline-bundle`.
5. Click `Set Up EchoNote` or `Repair EchoNote`.

Companion fully verifies the bundle before changing the environment. It then runs the equivalent of:

```text
python -m pip install --no-index --disable-pip-version-check --no-cache-dir \
  --find-links <bundle>/wheels -r <bundle>/requirements-offline.txt
```

Models are copied into a staging directory, the previous model directory is renamed to a backup, and staging is atomically activated. If activation fails, the previous directory is restored.

The managed virtual environment is stored outside the signed application bundle at `~/Library/Application Support/EchoNote/runtime/.venv`, so upgrading `EchoNote.app` does not remove dependencies or invalidate the app signature.

At runtime Companion removes Hugging Face token variables and forces `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, `HF_DATASETS_OFFLINE=1`, and `PYTHONDONTWRITEBYTECODE=1`. The final setting prevents Python from creating `__pycache__` inside the signed application resources.

## Operational Notes

- After dependencies and models are installed, the source bundle may be unmounted; runtime remains available.
- Keep the exact bundle to support later repair.
- Changing ASR presets requires a bundle containing that preset and another setup/repair run.
- A corrupt bundle is rejected before the installed model directory is replaced.
