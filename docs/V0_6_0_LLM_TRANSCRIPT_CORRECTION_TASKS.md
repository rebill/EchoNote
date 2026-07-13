# EchoNote v0.6.0 LLM Transcript Correction Tasks

## Docs

- [x] Add v0.6.0 PRD.
- [x] Add v0.6.0 tech design.
- [x] Add v0.6.0 implementation task list.
- [x] Update plugin README.
- [x] Update changelog.

## Plugin Settings And Status

- [x] Add `enableLlmTranscriptCorrection` setting with default `false`.
- [x] Migrate/normalize loaded settings.
- [x] Add settings UI toggle with privacy copy.
- [x] Add `transcriptCorrection` and `transcriptCorrectionMessage` status fields.
- [x] Show transcript correction state in status view.

## LLM Provider

- [x] Add generic `generateText` provider method.
- [x] Implement for OpenAI-compatible provider.
- [x] Implement for Anthropic provider.
- [x] Keep Summary behavior unchanged.

## Correction Service

- [x] Add structured correction prompts.
- [x] Batch turns without splitting a turn.
- [x] Parse JSON response.
- [x] Validate turn count, id order, non-empty text, and length ratio.
- [x] Preserve rejected turns.
- [x] Apply deterministic correction rules before and after LLM.

## Note Writing

- [x] Save before-LLM transcript artifacts under `.echonote-artifacts/`.
- [x] Avoid overwriting artifact files.
- [x] Parse current Markdown transcript into turns for manual command.
- [x] Update metadata with LLM correction timestamp.

## Workflow

- [x] Run automatic correction after successful ASR final transcript write when enabled.
- [x] Do not run automatic correction for live chunks.
- [x] Add manual command `EchoNote: Correct Transcript with LLM`.
- [x] Keep ASR final transcript if LLM correction fails.
- [x] Surface missing LLM config errors clearly.

## Verification

- [x] Add unit tests.
- [x] Run plugin tests.
- [x] Run plugin typecheck.

## Release Closure

- [x] Align v0.6.0 version metadata across plugin, ASR service, Companion, and `versions.json`.
- [x] Add the CPU-bounded speaker diarization follow-up and serialize concurrent finalization.
- [x] Require and detect the compatible `pyannote.audio` 4.x dependency.
- [x] Record the source-only Companion release decision.
- [x] Build and package the Obsidian plugin release assets.
- [x] Run plugin, ASR service, Companion, fake-backend, and real diarization verification.
