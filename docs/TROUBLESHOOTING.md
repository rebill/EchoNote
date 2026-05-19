# EchoNote Troubleshooting

## ASR service did not become healthy: Failed to fetch

Likely causes:

- ASR service is not running.
- Wrong ASR service port.
- ASR service path or Python path is wrong.
- Old ASR service version without CORS support is still running.

Check:

```bash
curl http://127.0.0.1:8765/health
```

If this works in terminal but Obsidian still shows `Failed to fetch`, restart the ASR service and make sure the current code includes CORS middleware.

## Python path error

Use an absolute Python path in EchoNote settings:

```text
/Users/br/Git/github/rebill/EchoNote/asr-service/.venv/bin/python
```

Do not rely on `python3` unless you know Obsidian can resolve the same shell environment.

## ASR service path error

Use an absolute ASR service path:

```text
/Users/br/Git/github/rebill/EchoNote/asr-service
```

The default relative path may not work after the plugin is copied into an Obsidian vault.

## Model is not_loaded

Load the model:

```bash
curl -X POST http://127.0.0.1:8765/model/load \
  -H 'Content-Type: application/json' \
  -d '{"model_id":"mlx-community/Qwen3-ASR-0.6B-4bit"}'
```

Then check:

```bash
curl http://127.0.0.1:8765/model/status
```

Expected:

```json
{"status":"ready"}
```

## Transcript still says fake transcript

The ASR service is running with:

```text
--backend fake
```

Restart it with:

```bash
python -m echonote_asr \
  --host 127.0.0.1 \
  --port 8765 \
  --model mlx-community/Qwen3-ASR-0.6B-4bit \
  --backend mlx-audio \
  --log-level info
```

## STTOutput appears in Transcript

This was caused by an empty ASR result being stringified.

Fix:

- Restart the ASR service with the latest code.
- Rebuild and reload the plugin if needed.

Current code skips empty ASR text and does not write `STTOutput(...)` to the note.

## No meeting note is created

Check the status panel `Last Error`.

Common blockers before note creation:

- ASR service cannot be reached.
- Model did not reach `ready`.
- Meeting folder cannot be created.

After ASR is healthy and model is ready, EchoNote should show:

```text
EchoNote: creating meeting note...
```

## No audio from BlackHole

BlackHole is a virtual pipe. It only records sound routed into it.

Recommended route:

```text
System Output: Multi-Output Device
EchoNote Input: BlackHole 2ch
```

See [macOS Virtual Audio Guide](MAC_VIRTUAL_AUDIO_GUIDE.md).

## Cannot find Multi-Output Device

Open:

```text
Audio MIDI Setup
```

Then click the `+` button in the lower-left corner and choose:

```text
Create Multi-Output Device
```

See [macOS Virtual Audio Guide](MAC_VIRTUAL_AUDIO_GUIDE.md).

## Loopback device not visible

Check:

- Loopback virtual device is enabled.
- The device appears in macOS Sound Input.
- Obsidian has microphone permission.
- EchoNote settings page has refreshed audio input devices.
- Obsidian has been restarted after creating the device.

## AI Summary fails

Check:

- Correct LLM provider is selected.
- API key is configured.
- Model is configured.
- Base URL is configured for OpenAI-compatible provider.
- The meeting note contains a non-empty `## Transcript` section.

EchoNote does not write summary sections if the LLM response cannot be parsed as JSON.

