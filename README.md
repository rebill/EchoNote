# EchoNote

EchoNote is a desktop app for meeting transcription. It records your microphone and the system audio as two tracks, transcribes them on your computer, keeps the meetings in a local library, and can summarize them with an LLM provider you configure.

Status: `1.0.0-alpha.0`. The installers are unsigned: macOS builds are ad-hoc signed and not notarized, and Windows builds are not signed. Windows capture, the Windows installer, macOS 14.2 to 14.3 and the 3-hour soak test have not run on real hardware yet (see the known limitations in [CHANGELOG.md](CHANGELOG.md)). This README covers the v1 desktop app. The earlier Obsidian plugin is described under [Legacy code](#legacy-code).

## Features

- Two tracks recorded at once, with no virtual audio device. The microphone is labeled "我" (me) and the system audio "对方" (the other side). System audio comes from a Core Audio process tap on macOS and WASAPI loopback on Windows.
- Live transcription on the CPU with sherpa-onnx. The default model (SenseVoice-Small int8) and the voice activity detector (Silero VAD) ship in the installer, so recording and transcription work offline.
- A meeting library in SQLite: substring search (including Chinese), rename, delete, playback from any transcript line, per-line edits with revert, and a glossary.
- Export to Markdown or plain text. Markdown is an export format only; the SQLite library is the store.
- AI features: summaries from built-in or custom templates, Q&A with timestamp links, transcript correction, and optional live highlights while recording. They work with any OpenAI-compatible endpoint (presets include OpenAI, DeepSeek, Qwen, Kimi, Zhipu GLM, Ollama and LM Studio) or the Anthropic Messages API. API keys are stored in the macOS Keychain or the Windows Credential Manager.
- An optional high-accuracy model, Qwen3-ASR 0.6B int8 (about 990 MB), downloaded from Settings on request. It re-transcribes a saved meeting, and the previous version can be restored.
- UI in Simplified Chinese (default) and English.

## Install

| Platform | Installer |
| --- | --- |
| macOS 14.2 or later, Apple silicon | `aarch64` DMG |
| macOS 14.2 or later, Intel | `x64` DMG |
| Windows 10 or 11, x64 | NSIS `*-setup.exe`, installed per user without administrator rights |

Tagged releases attach the installers and `SHA256SUMS.txt` to a GitHub release (see [CI and releases](#ci-and-releases)), or you can build them yourself (see [Release build](#release-build)). Because the builds are unsigned, Gatekeeper and SmartScreen block the first launch. [docs/V1_0_0_DESKTOP_APP_INSTALL.md](docs/V1_0_0_DESKTOP_APP_INSTALL.md) explains how to verify the checksums, allow the app, grant permissions, uninstall, and build a self-signed copy.

## Privacy

Audio, transcripts and the library stay on your computer. There is no account and no telemetry. The app goes online only when you download a model in Settings or use an AI feature. In that case the meeting's transcript text, never the audio, goes to the provider you configured. Before the first send to a non-local provider the app asks you to confirm, and it asks again if the provider's scheme, host or port changes.

Data directory: `~/Library/Application Support/com.rebill.echonote` on macOS, `%LOCALAPPDATA%\com.rebill.echonote` on Windows. See [docs/V1_0_0_DESKTOP_APP_PRIVACY.md](docs/V1_0_0_DESKTOP_APP_PRIVACY.md).

## Repository layout

```text
app/                    Desktop app: Svelte 5 frontend (app/src) and Tauri 2 Rust shell (app/src-tauri)
crates/echonote-core    Capture, VAD, segmenter, ASR (sherpa-onnx, linked statically), SQLite store,
                        Ogg/Opus recording, export, engine. No Tauri dependency.
crates/echonote-llm     OpenAI-compatible and Anthropic streaming clients; summary, Q&A, correction
crates/echonote-models  Model manager: manifest, resumable verified downloads, atomic install
crates/echonote-opus    Vendored libopus with a minimal encoder and decoder
crates/echonote-bench   ASR benchmarks and the regression gates CI runs
scripts/dev/            Development, packaging and test scripts
third_party/            Inputs for the third-party notices
docs/                   v1 product, design and user documents (V1_0_0_*), plus legacy documents
plugin/ companion/ asr-service/   Frozen v0.x product, see Legacy code below
```

## Build from source

### Prerequisites

- Rust stable, 1.90 or later (required by the Tauri app crate).
- Node.js (CI uses 22) and pnpm 10.11.0 (pinned in `app/package.json`).
- Python 3 for `scripts/dev/`. The scripts use only the standard library.
- The [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS: Xcode Command Line Tools on macOS; Microsoft C++ Build Tools ("Desktop development with C++") and WebView2 on Windows.

The commands below use POSIX shell syntax. In PowerShell, set variables with `$env:NAME = "value"`.

### sherpa-onnx native library

`echonote-core` links sherpa-onnx 2.0.1 statically. If `SHERPA_ONNX_ARCHIVE_DIR` is not set, the `sherpa-onnx-sys` build script downloads the archive without checking it. Fetch the pinned archive once, which verifies its SHA-256, and point the build at the folder with an absolute path:

```sh
python3 scripts/dev/fetch_sherpa_onnx.py "$HOME/.cache/echonote-dev/sherpa-onnx"
export SHERPA_ONNX_ARCHIVE_DIR="$HOME/.cache/echonote-dev/sherpa-onnx"
```

The target defaults to the host. Pass a Rust target as the second argument to fetch another one: `aarch64-apple-darwin`, `x86_64-apple-darwin` or `x86_64-pc-windows-msvc`.

### Development models

Debug builds also look for models in `$ECHONOTE_DEV_DATA/models` (default `~/.cache/echonote-dev/models`). Fetch the live model and the VAD, and optionally the high-accuracy model:

```sh
python3 scripts/dev/fetch_assets.py sense-voice-int8 vad   # about 240 MB
python3 scripts/dev/fetch_assets.py qwen3-asr-int8         # optional, about 990 MB
```

Every file is checked against the SHA-256 in `crates/echonote-bench/assets.json`. `--list` prints all asset ids. Running the script with no arguments fetches everything, about 5.5 GB including the benchmark evaluation sets.

### Run

```sh
cd app && pnpm install && pnpm tauri dev
```

This starts the Vite dev server on `127.0.0.1:1420` and a debug build of the app. Debug builds keep API keys under a separate keychain service (`com.rebill.echonote.debug`), but they use the same data directory as the installed app. Set `ECHONOTE_DATA_DIR` to keep test meetings out of your real library.

Debug builds read these variables; release builds ignore them. The full list is in `app/src-tauri/src/dev.rs`.

| Variable | Effect |
| --- | --- |
| `ECHONOTE_DATA_DIR` | Library and recordings directory |
| `ECHONOTE_FAKE_INPUT` | Directory with `mic.wav` (and optionally `sys.wav`), played in real time instead of the devices |
| `ECHONOTE_MODEL_MIRROR` | Base URL that replaces every model download source |

`scripts/dev/app_smoke.sh` and `scripts/dev/soak.sh` drive the real UI with fake input in a scratch library. Their header comments list what they need.

### Test

The app crate embeds `app/dist`, so build the frontend before running the Rust tests:

```sh
(cd app && pnpm install && pnpm check && pnpm test && pnpm build)
cargo test --workspace
(cd scripts/dev && python3 -m unittest discover -s tests)
```

CI also runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo run --locked -p echonote-bench -- gate`. The gate runs deterministic regression checks on synthetic fixtures and needs no models.

### Release build

From the repository root, in the order `release.yml` uses. The release checklist is in [docs/V1_0_0_DESKTOP_APP_RELEASE.md](docs/V1_0_0_DESKTOP_APP_RELEASE.md).

```sh
python3 scripts/dev/stage_bundle_models.py
(cd app && pnpm install --frozen-lockfile)
python3 scripts/dev/gen_notices.py
(cd app && pnpm bundle --target aarch64-apple-darwin --bundles app,dmg)
python3 scripts/dev/check_bundle.py target/aarch64-apple-darwin/release aarch64-apple-darwin
```

| Installer | `--target` | `--bundles` |
| --- | --- | --- |
| macOS arm64 DMG | `aarch64-apple-darwin` | `app,dmg` |
| macOS x64 DMG | `x86_64-apple-darwin` | `app,dmg` |
| Windows x64 NSIS | `x86_64-pc-windows-msvc` | `nsis` |

- `stage_bundle_models.py` copies the live model and VAD files listed in `crates/echonote-models/manifest.json` into `app/src-tauri/resources/models/` and checks each SHA-256. It copies from the dev models when they match and downloads otherwise; `--offline` fails instead of downloading.
- `gen_notices.py` writes `app/src-tauri/resources/THIRD_PARTY_NOTICES.txt` from `third_party/notices.json`, `app/node_modules` and `cargo metadata`, so run it after `pnpm install`.
- `pnpm bundle` runs `tauri build` with `app/src-tauri/tauri.bundle.conf.json`, which adds the models, the notices and `License` (as `LICENSE.txt`). Output goes to `target/<target>/release/bundle/`.
- `check_bundle.py` checks the bundled models and notices and the 300 MiB installer budget (`app.installerMiB` in `docs/v1-performance-budgets.json`). It prints a JSON report and exits with status 1 on any failure.
- For a target other than the host, run `rustup target add <target>` and fetch that target's sherpa-onnx archive first. CI builds the macOS x64 DMG this way on an Apple silicon runner.

## CI and releases

- `.github/workflows/desktop.yml` runs the frontend checks, script tests, format, clippy, Rust tests and bench gates on macOS arm64 and Windows x64.
- `.github/workflows/release.yml` runs the same checks, then builds the three installers. A pushed `v1.*` tag must match the version in `app/package.json`. The tag creates a draft GitHub release with the installers and `SHA256SUMS.txt`, marked as a pre-release when the tag has a suffix such as `-alpha.0`. Publishing the draft is manual. A manual run of the workflow uploads the installers as workflow artifacts only.
- `.github/workflows/ci.yml` tests the legacy plugin and ASR service.

## Documentation

The v1 documents are in Simplified Chinese.

- [Installation](docs/V1_0_0_DESKTOP_APP_INSTALL.md): system requirements, checksums, Gatekeeper and SmartScreen, self-signed builds
- [Privacy](docs/V1_0_0_DESKTOP_APP_PRIVACY.md)
- [Troubleshooting](docs/V1_0_0_DESKTOP_APP_TROUBLESHOOTING.md)
- [Product requirements](docs/V1_0_0_DESKTOP_APP_PRD.md)
- [Technical design](docs/V1_0_0_DESKTOP_APP_TECH_DESIGN.md)
- [Tasks and milestones](docs/V1_0_0_DESKTOP_APP_TASKS.md)
- [Release process](docs/V1_0_0_DESKTOP_APP_RELEASE.md)
- [Changelog](CHANGELOG.md): the `1.0.0-alpha.0` entry and its known limitations

## Legacy code

`plugin/`, `companion/` and `asr-service/` are the v0.x product: an Obsidian plugin, a Tauri companion app, and a local Python ASR service. They are frozen. They will be removed after v1 ships, once the removal is approved separately ([docs/V1_0_0_LEGACY_REMOVAL_PROPOSAL.md](docs/V1_0_0_LEGACY_REMOVAL_PROPOSAL.md)). `CONTRIBUTING.md`, `versions.json`, the scripts outside `scripts/dev/`, and the documents in `docs/` without a `V1_0_0_` or `v1-` prefix describe that product. `CHANGELOG.md` covers both.

## Third-party notices

[NOTICE](NOTICE) lists the bundled models and the linked native libraries. Each installer includes `THIRD_PARTY_NOTICES.txt` with every license text, generated by `scripts/dev/gen_notices.py`. The bundled SenseVoice model is under the FunASR Model Open Source License 1.1, not under EchoNote's license.

## License

EchoNote is source-available under the PolyForm Noncommercial License 1.0.0.

Personal, educational, and non-commercial use is allowed.

Commercial use requires a separate commercial license. See [License](License).
