# EchoNote Obsidian Plugin

This directory contains the Obsidian TypeScript plugin.

## Install

```bash
npm install
```

## Build

```bash
npm run build
```

Build output:

```text
main.js
manifest.json
styles.css
```

## Type Check

```bash
npm run typecheck
```

## ASR Runtime

v0.2.0 uses EchoNote ASR Companion as the only ASR backend. The Obsidian plugin no longer starts or restarts its own Python ASR process.

The default discovery file is:

```text
~/Library/Application Support/EchoNote/companion.json
```

Start, stop, restart, model loading, and ASR logs are managed in the Companion app. The plugin only reads the discovery file and calls the Companion-managed localhost ASR API.

## Manual Vault Install

```bash
mkdir -p "/path/to/Vault/.obsidian/plugins/echonote"
cp main.js manifest.json styles.css "/path/to/Vault/.obsidian/plugins/echonote/"
```

Reload Obsidian or disable/enable EchoNote after copying.
