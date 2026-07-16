import { performance } from "node:perf_hooks";
import type { TranscriptTurn } from "../src/asr/asr-types";
import { MeetingAudioSpool } from "../src/audio/meeting-audio-spool";
import { concatWavFiles } from "../src/audio/wav-encoder";
import {
  extractTranscript,
  isEchoNoteMeetingNote,
  replaceSummarySections
} from "../src/meeting/markdown-sections";
import { formatTranscriptTurns } from "../src/meeting/transcript-markdown";
import { sanitizeTranscriptText } from "../src/meeting/transcript-sanitizer";

type BenchmarkResult = {
  name: string;
  iterations: number;
  medianMs: number;
  p95Ms: number;
  minMs: number;
  maxMs: number;
};

type AudioSpoolBenchmarkResult = {
  name: string;
  minutes: number;
  chunks: number;
  storageMode: "memory" | "disk";
  appendMs: number;
  assemblyMs: number;
  outputMiB: number;
  retainedArrayBufferDeltaMiB: number;
};

const transcriptLines = Array.from({ length: 20_000 }, (_, index) =>
  `[${formatOffset(index * 3_000)}] Speaker ${(index % 4) + 1}: 第 ${index + 1} 条性能基准转录，讨论产品、工程和后续行动。`
);
const transcript = transcriptLines.join("\n");
const meetingNote = `<!-- echonote-meeting -->
# Performance baseline

## Summary

_Pending._

## Decisions

_Pending._

## Action Items

_Pending._

## Key Points

_Pending._

## Open Questions

_Pending._

## Transcript

${transcript}
`;
const turns: TranscriptTurn[] = Array.from({ length: 10_000 }, (_, index) => ({
  id: `turn-${index + 1}`,
  text: `Turn ${index + 1} discusses performance and a concrete follow-up action.`,
  speaker: `Speaker ${(index % 4) + 1}`,
  started_at_ms: index * 3_000,
  ended_at_ms: (index + 1) * 3_000,
  confidence: 0.9
}));
const pathologicalTranscript = "EchoNote performance regression sentence. ".repeat(500);
const wavChunks = Array.from({ length: 40 }, () => new ArrayBuffer(44 + (16_000 * 15 * 2)));

void main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});

async function main(): Promise<void> {
  const results: BenchmarkResult[] = [
    runBenchmark("markdown.extract_transcript.20k_lines", 20, () => extractTranscript(meetingNote)),
    runBenchmark("markdown.verify_meeting.20k_lines", 20, () => isEchoNoteMeetingNote(meetingNote)),
    runBenchmark("markdown.replace_summary.20k_lines", 20, () => replaceSummarySections(meetingNote, {
      Summary: "Performance summary",
      Decisions: "- Keep benchmark gates",
      "Action Items": "- [ ] Optimize hot paths",
      "Key Points": "- Measure before changing",
      "Open Questions": "- None"
    })),
    runBenchmark("transcript.format.10k_turns", 10, () => formatTranscriptTurns(turns, true, "")),
    runBenchmark("transcript.sanitize.pathological", 50, () => sanitizeTranscriptText(pathologicalTranscript)),
    runBenchmark("wav.concat.10_minutes", 5, () => concatWavFiles(wavChunks))
  ];
  const audioSpool = [];
  for (const minutes of [10, 30, 60]) {
    audioSpool.push(await runAudioSpoolBenchmark(minutes));
  }

  console.log(JSON.stringify({
    runtime: {
      node: process.version,
      platform: process.platform,
      arch: process.arch
    },
    inputs: {
      noteChars: meetingNote.length,
      transcriptLines: transcriptLines.length,
      transcriptTurns: turns.length,
      wavChunks: wavChunks.length,
      wavMinutes: 10
    },
    results,
    audioSpool
  }, null, 2));
}

async function runAudioSpoolBenchmark(minutes: number): Promise<AudioSpoolBenchmarkResult> {
  const chunks = minutes * 4;
  const chunk = new ArrayBuffer(44 + (16_000 * 15 * 2));
  const spool = new MeetingAudioSpool();
  global.gc?.();
  const initialArrayBuffers = process.memoryUsage().arrayBuffers;
  const appendStartedAt = performance.now();
  for (let index = 0; index < chunks; index += 1) {
    await spool.append(chunk);
  }
  const appendMs = performance.now() - appendStartedAt;
  const storageMode = spool.storageMode;
  const assemblyStartedAt = performance.now();
  const wav = await spool.toWav();
  const assemblyMs = performance.now() - assemblyStartedAt;
  const retainedArrayBufferDelta = Math.max(0, process.memoryUsage().arrayBuffers - initialArrayBuffers);
  const result = {
    name: `audio_spool.stop.${minutes}_minutes`,
    minutes,
    chunks,
    storageMode,
    appendMs: round(appendMs),
    assemblyMs: round(assemblyMs),
    outputMiB: round((wav?.byteLength ?? 0) / (1024 * 1024)),
    retainedArrayBufferDeltaMiB: round(retainedArrayBufferDelta / (1024 * 1024))
  };
  await spool.dispose();
  return result;
}

function runBenchmark(name: string, iterations: number, operation: () => unknown): BenchmarkResult {
  for (let index = 0; index < 3; index += 1) {
    operation();
  }
  global.gc?.();

  const durations: number[] = [];
  for (let index = 0; index < iterations; index += 1) {
    const startedAt = performance.now();
    operation();
    durations.push(performance.now() - startedAt);
  }
  durations.sort((left, right) => left - right);

  return {
    name,
    iterations,
    medianMs: round(percentile(durations, 0.5)),
    p95Ms: round(percentile(durations, 0.95)),
    minMs: round(durations[0]),
    maxMs: round(durations[durations.length - 1])
  };
}

function percentile(values: number[], ratio: number): number {
  const index = Math.min(values.length - 1, Math.max(0, Math.ceil(values.length * ratio) - 1));
  return values[index];
}

function round(value: number): number {
  return Math.round(value * 1_000) / 1_000;
}

function formatOffset(offsetMs: number): string {
  const totalSeconds = Math.floor(offsetMs / 1_000);
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;
  return [hours, minutes, seconds].map((value) => String(value).padStart(2, "0")).join(":");
}
