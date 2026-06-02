# EchoNote Privacy Notes

## 1. Local ASR

EchoNote MVP uses local ASR by default.

The ASR service runs on:

```text
127.0.0.1
```

Meeting audio is sent from the Obsidian plugin to the local ASR service on the same machine.

EchoNote MVP does not send meeting audio to cloud ASR services.

## 2. Model Download

The first time you use `mlx-community/Qwen3-ASR-0.6B-4bit`, MLX / Hugging Face tooling may download model files from the internet.

If speaker diarization is enabled, `pyannote.audio` may download `pyannote/speaker-diarization-community-1` after you configure a Hugging Face token and accept the model terms.

After models are cached locally, ASR inference and diarization run on your machine.

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

EchoNote desktop stores the Hugging Face token used for optional speaker diarization in its local Companion settings file. The token is passed to the local ASR service through an environment variable and must not be written to discovery files, diagnostics, or logs.

Do not share your vault configuration files if they contain API keys.

Future versions may move secrets to macOS Keychain.

## 7. Virtual Audio Devices

If you use BlackHole or Loopback, EchoNote records whatever that selected input device receives.

Review your audio routing carefully. If your virtual device includes meeting software output, EchoNote can record that output. If it includes microphone input, EchoNote can record your microphone.
