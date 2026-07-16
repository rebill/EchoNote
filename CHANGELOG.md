# Changelog

All notable changes to EchoNote are documented in this file.

This project follows semantic versioning for release tags.

## [0.8.0] - 2026-07-16

### Added

- Added reproducible plugin and ASR performance benchmarks with version-controlled budgets.
- Added summary progress reporting, ASR phase timings, and diarization finalization timings.
- Added bounded disk spooling for long meeting audio and coalesced live-transcript writes.

### Changed

- Long transcripts now use boundary-aware chunks, two concurrent partial summaries, targeted retries, and bounded hierarchical merges.
- WAV assembly now uses bulk byte copies and assembles the complete recording once for saving and finalization.
- ASR inference is serialized in a reusable temporary workspace with cancellation-safe cleanup.
- Speaker assignment now uses a sorted sweep/window candidate set while preserving turn order and speaker-label semantics.
- Transcript formatting parses correction rules once per batch and skips unnecessary repeated-run comparisons.
- Version metadata is aligned for the v0.8.0 plugin, ASR service, Companion, and Obsidian version map.

### Performance

- Ten-minute WAV concatenation improved from 55.555 ms to 1.864 ms median.
- Formatting 10,000 transcript turns improved from 52.140 ms to 31.202 ms median.
- Assigning 2,000 turns against 4,000 speaker intervals improved from 810.371 ms to 5.545 ms median.
- Reused ASR temporary I/O reduced median overhead from 0.546 ms to 0.254 ms.

## [0.7.1] - 2026-07-16

### Fixed

- Reject incomplete or incorrectly typed meeting-summary JSON before changing a note.
- Verify meeting-note markers before acting on an active Markdown file and fall back to the last tracked meeting.
- Prevent concurrent summary requests and distinguish LLM response errors from note write failures.
- Roll back the note rename when summary content cannot be written, and surface saved-artifact migration warnings.
- Run plugin unit tests in CI and discover new test files automatically.

## [0.7.0] - 2026-07-13

### Changed

- Meeting summaries now generate a concise meeting topic and rename both the Obsidian note file and its level-one heading to `YYYY-MM-DD_会议主题`.
- Saved meeting audio and transcript-segment artifacts follow the summarized meeting title when possible.

## [0.6.0] - 2026-07-13

### Added

- Added optional LLM transcript correction after finalized ASR transcripts are written.
- Added a manual `EchoNote: Correct Transcript with LLM` command for the current meeting note.
- Added before-LLM transcript artifacts under `.echonote-artifacts/`.
- Added transcript correction status to the Obsidian status panel.
- Added v0.6.0 PRD, technical design, and task documents for LLM transcript correction.

### Changed

- LLM providers now expose a shared text-generation method used by summaries and transcript correction.
- CPU speaker diarization now uses a balanced sliding-window stride, a bounded PyTorch/BLAS thread budget, and serialized finalization to reduce CPU load and prevent retry overlap.
- Optional speaker diarization now requires `pyannote.audio` 4.x, and Companion setup detects and repairs incompatible installed versions.
- Companion v0.6.0 remains source-only; no unsigned macOS app artifact is included as a release asset.
- Version metadata is aligned for the v0.6.0 plugin, ASR service, Companion, and Obsidian version map.

### Fixed

- Prevented queued or cancelled speaker-finalization requests from starving the shared ASR worker pool or deleting temporary audio before background inference finishes.

## [0.5.0] - 2026-06-03

### Added

- Added user-maintained transcript correction rules in the Obsidian plugin settings.
- Added configurable automatic meeting stop after a long silent period, enabled by default at 10 minutes.
- Added saved transcript segment artifacts next to saved raw meeting audio.
- Added an Obsidian command to re-finalize the current meeting transcript with speaker labels from saved audio.

### Changed

- Live and finalized transcript text now applies configured ASR correction rules before writing meeting notes.
- Speaker finalization can now be retried after a pyannote timeout or failure when raw audio saving was enabled.
- Companion v0.5.0 remains source-only; no unsigned macOS app artifact is included as a release asset.
- Version metadata is aligned for the v0.5.0 plugin, ASR service, Companion, and Obsidian version map.

### Fixed

- Fixed stale Companion discovery after the desktop app stayed open without UI refreshes by adding a backend discovery heartbeat.

## [0.4.0] - 2026-06-01

### Added

- Added silence-aware audio chunking for live transcription with a 15 second force-cut fallback.
- Added transcript turns, speaker labels, and `/transcript/finalize` for speaker-aware final transcripts.
- Added optional local `pyannote.audio` diarization support with Hugging Face token handling in EchoNote desktop.
- Added Companion discovery capabilities for adaptive chunking and speaker diarization.
- Added plugin, ASR service, and Companion tests for v0.4.0 transcript contracts and degradation paths.

### Changed

- Final meeting stop now attempts a local finalize pass and only replaces `## Transcript` when non-empty final turns are returned.
- Complete meeting audio is temporarily retained in memory for local finalization, while vault audio saving remains controlled by `Save raw audio`.
- Version metadata is aligned for the v0.4.0 plugin, ASR service, Companion, and Obsidian version map.

## [0.3.0] - 2026-05-22

### Added

- Added the EchoNote setup wizard for first-run detection, one-click fake-backend runtime setup/repair, default service start, setup reset, and setup-aware diagnostics.
- Added shared Rust and TypeScript setup response contracts, UI fixtures, and setup-aware fake-backend smoke coverage.

### Changed

- Initialized version metadata for the v0.3.0 plugin, ASR service, Companion, and Obsidian version map.
- Renamed the desktop app display name from `EchoNote ASR Companion` to `EchoNote`.
- Renamed Companion package, crate, and local preview identifiers to `echonote`.
- Moved Python path, ASR service path, port, backend, and custom model ID controls behind `Advanced Settings`.
- Updated Obsidian plugin runtime copy to direct users to open EchoNote and click `Set Up EchoNote` or `Start Service`.

## [0.2.0] - 2026-05-21

### Added

- macOS EchoNote ASR Companion app for starting, stopping, monitoring, and diagnosing the local ASR service.
- Companion-only ASR runtime in the Obsidian plugin.
- Companion discovery via `~/Library/Application Support/EchoNote/companion.json`.
- Companion logs and diagnostic report support under `~/Library/Logs/EchoNote`.
- Fake-backend smoke test for ASR health/model/transcription, Companion discovery, and plugin runtime resolution.
- Grouped, responsive Obsidian status sidebar styling for clearer runtime, Companion, and session diagnostics.

### Changed

- Version metadata is aligned for the v0.2.0 plugin, ASR service, Companion, and Obsidian version map.
- Companion v0.2.0 is released as source-only until a signed and verified macOS app artifact is available.

## [0.1.0] - 2026-05-20

### Added

- Initial EchoNote Obsidian desktop plugin release.
- Local ASR service with fake and MLX audio backends.
- Quasi-real-time transcript chunking into Markdown meeting notes.
- OpenAI-compatible and Anthropic meeting summary providers.
- Release assets for manual Obsidian installation: `main.js`, `manifest.json`, `README.md`, and `echonote-v0.1.0.zip`.

### Fixed

- Collapse runaway repeated ASR transcript text before writing meeting notes.
- Add ASR service and plugin-side transcript sanitization guards for pathological repeated output.

[0.8.0]: https://github.com/rebill/EchoNote/compare/v0.7.1...v0.8.0
[0.7.1]: https://github.com/rebill/EchoNote/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/rebill/EchoNote/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/rebill/EchoNote/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/rebill/EchoNote/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/rebill/EchoNote/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/rebill/EchoNote/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/rebill/EchoNote/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/rebill/EchoNote/releases/tag/v0.1.0
