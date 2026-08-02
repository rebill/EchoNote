# EchoNote Privacy Notes

## 1. Local ASR

EchoNote MVP uses local ASR by default.

The ASR service runs on:

```text
127.0.0.1
```

Meeting audio is sent from the Obsidian plugin to the local ASR service on the same machine.

EchoNote MVP does not send meeting audio to cloud ASR services.

## 2. Offline Models and Installation

EchoNote v0.9.0 does not download ASR or speaker diarization models at runtime. Companion installs authorized local model files from a transferred offline bundle after checking every file's size and SHA-256.

Dependency installation uses a local wheelhouse with `pip --no-index`. The ASR process forces Hugging Face and Transformers offline modes and removes Hugging Face token variables.

The connected machine used to create the bundle may download packages and model files. Model licenses and redistribution permissions remain the bundle creator's responsibility.

## 3. Raw Audio Storage

Raw meeting audio is not saved by default.

If you enable:

```text
Save raw audio
```

EchoNote saves one complete WAV file per meeting in the configured audio folder.

Example:

```text
Meetings/audio/2026-05-19 13-52 Meeting/2026-05-19 13-52 Meeting.wav
```

EchoNote does not save individual chunk WAV files in the current MVP.

For v0.4.0 speaker-aware final transcripts, EchoNote temporarily keeps complete meeting audio in memory while a meeting is active. This in-memory audio is sent only to the local ASR service on `127.0.0.1` and is released after stop/finalize completes. It is not written to the vault unless `Save raw audio` is enabled.

## 4. Meeting Notes

Meeting notes are saved in your Obsidian vault as Markdown files.

Default folder:

```text
Meetings/
```

Transcript and AI summary are stored in the same note.

## 5. AI Summary Providers

If you use a cloud LLM provider, EchoNote sends the meeting transcript to that provider.

Supported MVP providers:

- OpenAI-compatible API.
- Anthropic API.

If you want summary generation to remain local, configure an OpenAI-compatible local endpoint.

## 6. API Keys

EchoNote MVP stores API keys in Obsidian plugin settings.

EchoNote desktop does not require or store a Hugging Face token. Legacy Companion token fields are removed when settings are migrated to v0.9.0.

Do not share your vault configuration files if they contain API keys.

Future versions may move secrets to macOS Keychain.

## 7. Virtual Audio Devices

If you use BlackHole or Loopback, EchoNote records whatever that selected input device receives.

Review your audio routing carefully. If your virtual device includes meeting software output, EchoNote can record that output. If it includes microphone input, EchoNote can record your microphone.
