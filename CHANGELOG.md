# Changelog

All notable changes to EchoNote are documented in this file.

This project follows semantic versioning for release tags.

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

[0.5.0]: https://github.com/rebill/EchoNote/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/rebill/EchoNote/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/rebill/EchoNote/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/rebill/EchoNote/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/rebill/EchoNote/releases/tag/v0.1.0
