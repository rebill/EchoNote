# EchoNote ASR Companion

macOS Tauri companion app for managing the local EchoNote ASR service.

This v0.2.0 desktop app provides a Vite/TypeScript frontend and Tauri 2 Rust backend for starting, stopping, monitoring, and diagnosing the local EchoNote ASR service.

v0.2.0 is source-only. The app can be built locally, but the project does not publish a signed or notarized `.app` / `.dmg` artifact for this release.

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

The app recovers missing or invalid settings files by writing safe defaults. The UI can load and save Python path, ASR service path, preferred port, backend, and model preset/custom model.
