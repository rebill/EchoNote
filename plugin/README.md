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

## LLM Transcript Correction

LLM transcript correction is experimental and disabled by default. When enabled, EchoNote sends the final transcript to the configured LLM provider after speaker finalization, applies conservative typo checks, then updates `## Transcript` only when text actually changes.

The correction keeps timestamps, speaker labels, and turn order intact. Before overwriting the transcript, EchoNote saves the previous transcript under `.echonote-artifacts/` next to the meeting note.

You can also run `EchoNote: Correct Transcript with LLM` on the current meeting note. This manual command works even if automatic LLM correction is disabled. For recurring names, projects, and technical terms, the deterministic `wrong => correct` rules are usually more reliable than LLM correction.

## Summary Titles

When a meeting summary succeeds, EchoNote renames the note file and its level-one heading to `YYYY-MM-DD_会议主题`. The meeting date comes from the note metadata when available, and the meeting topic comes from the structured LLM response.

EchoNote validates every required summary field before changing the note. It also verifies meeting-note markers before acting on an active Markdown file, preventing unrelated notes from being summarized or renamed accidentally.

## Long Meeting Performance

Long summaries use boundary-aware chunks, bounded concurrency, targeted retries, and hierarchical merges. Recording
audio stays in memory up to 32 MiB of PCM, then spills to a private temporary file; temporary audio is removed after
stop or startup failure. Live transcript writes are coalesced for up to 250 ms and are force-flushed when stopping.

Run `npm run benchmark:plugin` to measure the large-note, transcript-formatting, WAV, and audio-spool paths.

## Speaker Transcript Retry

When `Save raw audio` is enabled, EchoNote saves both the meeting WAV and a matching `*.segments.json` file in the configured audio folder.

If speaker finalization times out or fails, open the meeting note and run `EchoNote: Re-finalize Transcript with Speakers` from the Obsidian command palette. The command reloads the saved WAV and segments, calls the local ASR finalize endpoint again, and replaces `## Transcript` only when a non-empty finalized transcript is returned.

## Manual Vault Install

```bash
mkdir -p "/path/to/Vault/.obsidian/plugins/echonote"
cp main.js manifest.json styles.css "/path/to/Vault/.obsidian/plugins/echonote/"
```

Reload Obsidian or disable/enable EchoNote after copying.
