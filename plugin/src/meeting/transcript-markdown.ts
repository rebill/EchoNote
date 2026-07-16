import type { TranscriptTurn } from "../asr/asr-types";
import { applyParsedTranscriptCorrections, parseTranscriptCorrectionRules } from "./transcript-corrections";
import { sanitizeTranscriptText } from "./transcript-sanitizer";
import { formatTranscriptTimestamp } from "../utils/time";

export type ParsedTranscript = {
  turns: TranscriptTurn[];
  hasTimestamps: boolean;
};

export function parseTranscriptMarkdown(markdown: string): ParsedTranscript {
  const turns: TranscriptTurn[] = [];
  let hasTimestamps = false;

  for (const rawLine of markdown.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) {
      continue;
    }

    const parsedLine = parseTranscriptLine(line, turns.length);
    hasTimestamps = hasTimestamps || parsedLine.hasTimestamp;
    turns.push(parsedLine.turn);
  }

  return { turns, hasTimestamps };
}

export function formatTranscriptTurns(
  turns: TranscriptTurn[],
  enableTimestamps: boolean,
  correctionRules: string
): string {
  const lines: string[] = [];
  const parsedRules = parseTranscriptCorrectionRules(correctionRules);
  for (const turn of turns) {
    const text = applyParsedTranscriptCorrections(sanitizeTranscriptText(turn.text), parsedRules);
    if (!text) {
      continue;
    }

    const timestamp = enableTimestamps ? `[${formatTranscriptTimestamp(turn.started_at_ms)}] ` : "";
    const speaker = turn.speaker ? `${turn.speaker}: ` : "";
    lines.push(`${timestamp}${speaker}${text}`);
  }
  return lines.join("\n");
}

function parseTranscriptLine(line: string, index: number): { turn: TranscriptTurn; hasTimestamp: boolean } {
  const timestampMatch = line.match(/^\[([0-9]{2}:[0-9]{2}(?::[0-9]{2})?)\]\s*(.*)$/);
  const startedAtMs = timestampMatch ? parseTimestampMs(timestampMatch[1]) : 0;
  const content = timestampMatch ? timestampMatch[2].trim() : line;
  const speakerMatch = content.match(/^([^:：]{1,40})[:：]\s*(.*)$/);
  const speaker = speakerMatch && isGeneratedSpeakerLabel(speakerMatch[1]) ? speakerMatch[1].trim() : null;
  const text = speaker ? speakerMatch?.[2]?.trim() ?? "" : content;

  return {
    hasTimestamp: Boolean(timestampMatch),
    turn: {
      id: `manual-turn-${index + 1}`,
      text,
      speaker,
      started_at_ms: startedAtMs,
      ended_at_ms: startedAtMs,
      confidence: null
    }
  };
}

function parseTimestampMs(value: string): number {
  const parts = value.split(":").map((part) => Number.parseInt(part, 10));
  if (parts.some((part) => !Number.isInteger(part))) {
    return 0;
  }

  if (parts.length === 2) {
    return ((parts[0] * 60) + parts[1]) * 1000;
  }

  return ((parts[0] * 3600) + (parts[1] * 60) + parts[2]) * 1000;
}

function isGeneratedSpeakerLabel(value: string): boolean {
  return /^(speaker\s*\d+|speaker[_ -]?\d+|说话人\s*\d+|发言人\s*\d+)$/i.test(value.trim());
}
