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

EchoNote uses the EchoNote desktop app as the only ASR backend. The Obsidian plugin no longer starts or restarts its own Python ASR process.

The default discovery file is:

```text
~/Library/Application Support/EchoNote/companion.json
```

Start, stop, restart, model loading, and ASR logs are managed in the EchoNote desktop app. The plugin only reads the discovery file and calls the EchoNote-managed localhost ASR API. If discovery is missing, open EchoNote and click `Set Up EchoNote` or `Start Service`.

## Auto-Stop On Silence

EchoNote can automatically stop an active meeting after a long silent period. It is enabled by default at 10 minutes and can be changed in EchoNote settings.

## Transcript Corrections

Add user-maintained ASR correction rules in EchoNote settings. Use one rule per line:

```text
木溪 => 沐曦
Open AI => OpenAI
```

Rules apply to both live transcript chunks and the finalized transcript before meeting notes are written.

## Speaker Transcript Retry

When `Save raw audio` is enabled, EchoNote saves both the meeting WAV and a matching `*.segments.json` file in the configured audio folder.

If speaker finalization times out or fails, open the meeting note and run `EchoNote: Re-finalize Transcript with Speakers` from the Obsidian command palette. The command reloads the saved WAV and segments, calls the local ASR finalize endpoint again, and replaces `## Transcript` only when a non-empty finalized transcript is returned.

## Manual Vault Install

```bash
mkdir -p "/path/to/Vault/.obsidian/plugins/echonote"
cp main.js manifest.json styles.css "/path/to/Vault/.obsidian/plugins/echonote/"
```

Reload Obsidian or disable/enable EchoNote after copying.
