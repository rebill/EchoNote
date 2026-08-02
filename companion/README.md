# EchoNote

macOS Tauri desktop app for managing the local EchoNote ASR service.

This desktop app provides a Vite/TypeScript frontend and Tauri 2 Rust backend for starting, stopping, monitoring, and diagnosing the local EchoNote ASR service.

The release build embeds the ASR service source. Dependencies and authorized model weights are installed from an external, integrity-checked offline bundle; Python 3.11+ remains a prerequisite.

## Requirements

- Node.js 20+
- Rust stable
- Tauri 2 prerequisites for macOS

## Run Locally

```bash
cd companion
npm install
npm run tauri:dev
```

## Validate

```bash
cd companion
npm run typecheck
npm run build
cd src-tauri
cargo test
```

To build a local macOS `.app` for your own machine:

```bash
cd companion
npm run tauri:build
```

Do not treat the local `.app` as a release artifact unless it has been signed, notarized, and verified with Gatekeeper.

## Project Layout

```text
src/               Vite/TypeScript dashboard
src-tauri/src/     Rust process manager, discovery writer, logs, diagnostics, and Tauri commands
src-tauri/         Tauri config, capabilities, and Cargo project
```

## Settings

Settings are stored at:

```text
~/Library/Application Support/EchoNote/companion-settings.json
```

The app recovers missing or invalid settings files by writing safe defaults. The UI configures Python, the embedded ASR service, local ports, offline bundle/model paths, and ASR presets. Hugging Face tokens and remote model IDs are not used.
