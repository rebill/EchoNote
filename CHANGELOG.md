# Changelog

All notable changes to EchoNote are documented in this file.

This project follows semantic versioning for release tags.

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

[0.1.0]: https://github.com/rebill/EchoNote/releases/tag/v0.1.0
