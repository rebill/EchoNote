# EchoNote v0.6.0 LLM Transcript Correction Tech Design

## 1. Overview

v0.6.0 adds a plugin-side LLM transcript correction stage after ASR finalization. The ASR service remains unchanged. The Obsidian plugin reuses the existing LLM provider configuration and adds a strict structured correction contract.

Pipeline:

```text
Live chunks -> ASR transcribe -> live note append
Stop meeting -> ASR /transcript/finalize -> final turns -> write final transcript
If enabled -> save before artifact -> LLM correct turns -> validate -> replace transcript -> update metadata
```

## 2. Settings

Add to `EchoNoteSettings`:

```ts
enableLlmTranscriptCorrection: boolean;
```

Default: `false`.

Settings tab adds a toggle under the existing transcript correction section.

## 3. Status

Add a dedicated post-processing status:

```ts
type TranscriptCorrectionStatus = "idle" | "running" | "succeeded" | "failed";

type EchoNoteStatus = {
  transcriptCorrection: TranscriptCorrectionStatus;
  transcriptCorrectionMessage: string | null;
};
```

Do not reuse `speakerFinalization`.

## 4. LLM Provider Contract

Extend the provider interface with a text generation method:

```ts
type LlmTextRequest = {
  systemPrompt: string;
  userPrompt: string;
  temperature?: number;
};

interface LlmProvider {
  generateSummary(request: SummaryRequest): Promise<MeetingSummary>;
  generateText(request: LlmTextRequest): Promise<string>;
}
```

Summary continues to use its JSON-specific parsing. Transcript correction uses `generateText`.

## 5. Correction Input/Output

Input to LLM is JSON:

```json
{
  "turns": [
    {
      "id": "turn-1",
      "speaker": "Speaker 1",
      "started_at_ms": 0,
      "ended_at_ms": 12000,
      "text": "今天木溪会介绍..."
    }
  ]
}
```

Expected output:

```json
{
  "turns": [
    {
      "id": "turn-1",
      "text": "今天沐曦会介绍..."
    }
  ]
}
```

The LLM may only change `text`.

## 6. Chunking

Long transcripts are processed in batches:

- Do not split inside a turn.
- Batch by approximate character count.
- Recommended max batch size: 12000 chars.
- A failed batch keeps original turns for that batch.

## 7. Validation

For each batch:

- Output must be parseable JSON.
- `turns` length must match input length.
- `id` order must match.
- Corrected text cannot be empty when original text is non-empty.
- Per-turn text length ratio must stay within `0.4x` to `1.8x`, unless the original text is very short.

Rejected turns keep their original text. The service returns a result with applied/rejected counts.

## 8. Rule Ordering

Per turn:

```text
sanitize -> deterministic rules -> LLM correction -> deterministic rules
```

The existing user-maintained correction rules remain highest priority.

## 9. Note Writer Changes

`MeetingNoteWriter` adds:

- `saveTranscriptBeforeLlmArtifact(file, transcript): Promise<string>`
- `writeTranscriptCorrectionMetadata(file, date): Promise<void>`
- `parseTranscriptTurnsFromMarkdown(transcript): TranscriptTurn[]`

Metadata update inserts or replaces a top-level bullet near the existing metadata:

```md
- Transcript Correction: LLM corrected at 2026-06-04 14:30
```

The line must remain outside `## Transcript`.

## 10. Manual Command

Add command:

```text
EchoNote: Correct Transcript with LLM
```

The command:

1. Resolves current meeting note.
2. Reads `## Transcript`.
3. Parses markdown transcript lines into turns.
4. Saves before artifact.
5. Runs `TranscriptCorrectionService`.
6. Replaces `## Transcript`.
7. Updates metadata.

## 11. Error Handling

- Missing LLM config: fail with clear Notice/status error.
- LLM request failed: keep current transcript and show failure.
- Invalid LLM output: keep affected batch or turn original text.
- Empty transcript: show a clear message and skip.

## 12. Tests

Add unit tests for:

- Transcript markdown parsing.
- LLM correction JSON parsing and validation.
- Batch failure fallback.
- Metadata insertion/replacement.
- Artifact path generation behavior.
