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

After the model is cached locally, ASR inference runs on your machine.

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

Do not share your vault configuration files if they contain API keys.

Future versions may move secrets to macOS Keychain.

## 7. Virtual Audio Devices

If you use BlackHole or Loopback, EchoNote records whatever that selected input device receives.

Review your audio routing carefully. If your virtual device includes meeting software output, EchoNote can record that output. If it includes microphone input, EchoNote can record your microphone.

