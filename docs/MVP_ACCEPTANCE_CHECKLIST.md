# EchoNote MVP Acceptance Checklist

## Plugin

- [ ] Plugin builds with `npm run build`.
- [ ] Plugin type-checks with `npm run typecheck`.
- [ ] EchoNote can be enabled in Obsidian Desktop on macOS.
- [ ] Ribbon icon opens the status panel.
- [ ] Commands appear in the command palette.
- [ ] Settings persist after Obsidian restart.

## ASR Service

- [ ] ASR service starts on `127.0.0.1:8765`.
- [ ] `GET /health` returns `status: ok`.
- [ ] `POST /model/load` loads `mlx-community/Qwen3-ASR-0.6B-4bit`.
- [ ] `GET /model/status` returns `ready`.
- [ ] Fake ASR returns a valid `TranscriptSegment`.
- [ ] Real `mlx-audio` backend returns real text.

## Recording

- [ ] Microphone permission can be granted.
- [ ] Audio input device can be selected.
- [ ] BlackHole / Multi-Output Device path works when configured.
- [ ] Start Meeting creates a new note in `Meetings/`.
- [ ] Transcript is appended during recording.
- [ ] Pause stops new chunks.
- [ ] Resume continues chunk generation.
- [ ] Stop returns recording status to `idle`.
- [ ] Queue returns to `0 pending`.
- [ ] Short or silent tail chunks do not pollute Transcript.

## Audio Storage

- [ ] Raw audio is not saved by default.
- [ ] When enabled, one complete WAV is saved per meeting.
- [ ] No large set of chunk WAV files is created.

## AI Summary

- [ ] OpenAI-compatible provider can generate summary.
- [ ] Anthropic provider can generate summary.
- [ ] Summary updates only summary sections.
- [ ] Transcript is preserved.
- [ ] Invalid LLM JSON does not write broken summary content.

## Documentation

- [ ] README includes install, build, run, and workflow instructions.
- [ ] Privacy documentation exists.
- [ ] Troubleshooting documentation exists.
- [ ] Virtual audio setup documentation exists.
- [ ] M6 E2E test plan passes.

