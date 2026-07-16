import type { TranscriptSegment } from "../asr/asr-types";

const MAX_OVERLAP_TOKENS = 32;
const MIN_OVERLAP_TOKENS = 2;
const LEADING_SEPARATOR_PATTERN = /^[\s,，。.!！?？;；:：、—–-]+/u;

type TranscriptToken = {
  value: string;
  endIndex: number;
};

export function reconcileOverlappingTranscriptSegment(
  previous: TranscriptSegment | undefined,
  current: TranscriptSegment
): TranscriptSegment | null {
  if (!previous || current.started_at_ms >= previous.ended_at_ms) {
    return current;
  }

  const text = trimTranscriptPrefixOverlap(previous.text, current.text);
  if (!text) {
    return null;
  }

  const startedAtMs = Math.max(current.started_at_ms, previous.ended_at_ms);
  const turns = current.turns.length === 1
    ? [{
        ...current.turns[0],
        text,
        started_at_ms: Math.max(current.turns[0].started_at_ms, startedAtMs)
      }]
    : current.turns;

  return {
    ...current,
    text,
    turns,
    started_at_ms: startedAtMs
  };
}

export function trimTranscriptPrefixOverlap(previousText: string, currentText: string): string {
  const previousTokens = tokenizeTranscript(previousText);
  const currentTokens = tokenizeTranscript(currentText);
  const maxOverlap = Math.min(MAX_OVERLAP_TOKENS, previousTokens.length, currentTokens.length);

  for (let overlap = maxOverlap; overlap >= MIN_OVERLAP_TOKENS; overlap -= 1) {
    const previousStart = previousTokens.length - overlap;
    let matches = true;
    for (let index = 0; index < overlap; index += 1) {
      if (previousTokens[previousStart + index].value !== currentTokens[index].value) {
        matches = false;
        break;
      }
    }
    if (matches) {
      return currentText
        .slice(currentTokens[overlap - 1].endIndex)
        .replace(LEADING_SEPARATOR_PATTERN, "")
        .trim();
    }
  }

  return currentText.trim();
}

function tokenizeTranscript(text: string): TranscriptToken[] {
  const tokens: TranscriptToken[] = [];
  const pattern = /\p{Script=Han}|[\p{L}\p{N}]+/gu;
  for (const match of text.matchAll(pattern)) {
    const startIndex = match.index ?? 0;
    tokens.push({
      value: match[0].toLocaleLowerCase(),
      endIndex: startIndex + match[0].length
    });
  }
  return tokens;
}
