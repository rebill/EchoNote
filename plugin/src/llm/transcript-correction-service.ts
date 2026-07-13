import type { TranscriptTurn } from "../asr/asr-types";
import type { EchoNoteSettings } from "../settings/settings";
import { applyTranscriptCorrections } from "../meeting/transcript-corrections";
import { sanitizeTranscriptText } from "../meeting/transcript-sanitizer";
import { createLlmProvider } from "./llm-provider-factory";
import type { LlmProvider } from "./llm-types";

const MAX_BATCH_CHARS = 12000;
const MIN_RATIO_CHECK_CHARS = 20;
const MIN_LENGTH_RATIO = 0.4;
const MAX_LENGTH_RATIO = 1.8;

export type TranscriptCorrectionResult = {
  turns: TranscriptTurn[];
  acceptedTurns: number;
  changedTurns: number;
  rejectedTurns: number;
  failedBatches: number;
};

type CorrectionServiceOptions = {
  providerFactory?: (settings: EchoNoteSettings) => LlmProvider;
};

type CorrectionInputTurn = Pick<TranscriptTurn, "id" | "speaker" | "started_at_ms" | "ended_at_ms" | "text">;

type CorrectionResponseTurn = {
  id?: unknown;
  text?: unknown;
};

export class TranscriptCorrectionService {
  private readonly providerFactory: (settings: EchoNoteSettings) => LlmProvider;

  constructor(options: CorrectionServiceOptions = {}) {
    this.providerFactory = options.providerFactory ?? createLlmProvider;
  }

  async correctTurns(turns: TranscriptTurn[], settings: EchoNoteSettings): Promise<TranscriptCorrectionResult> {
    const normalizedTurns = turns
      .map((turn) => ({
        ...turn,
        text: applyTranscriptCorrections(sanitizeTranscriptText(turn.text), settings.transcriptCorrectionRules)
      }));

    if (normalizedTurns.length === 0) {
      return {
        turns: [],
        acceptedTurns: 0,
        changedTurns: 0,
        rejectedTurns: 0,
        failedBatches: 0
      };
    }

    const provider = this.providerFactory(settings);
    const correctedTurns: TranscriptTurn[] = [];
    let acceptedTurns = 0;
    let changedTurns = 0;
    let rejectedTurns = 0;
    let failedBatches = 0;

    for (const batch of batchTurns(normalizedTurns, MAX_BATCH_CHARS)) {
      try {
        const response = await provider.generateText({
          systemPrompt: buildCorrectionSystemPrompt(settings.summaryLanguage),
          userPrompt: buildCorrectionUserPrompt(batch),
          temperature: 0
        });
        const correctedBatch = parseCorrectionResponse(response);
        const validated = validateCorrectedBatch(batch, correctedBatch, settings.transcriptCorrectionRules);
        correctedTurns.push(...validated.turns);
        acceptedTurns += validated.acceptedTurns;
        changedTurns += validated.changedTurns;
        rejectedTurns += validated.rejectedTurns;
      } catch {
        correctedTurns.push(...batch);
        rejectedTurns += batch.length;
        failedBatches += 1;
      }
    }

    return {
      turns: correctedTurns,
      acceptedTurns,
      changedTurns,
      rejectedTurns,
      failedBatches
    };
  }
}

export function parseCorrectionResponse(rawText: string): CorrectionResponseTurn[] {
  const jsonText = extractJson(rawText);
  const parsed = JSON.parse(jsonText) as { turns?: unknown };
  if (!Array.isArray(parsed.turns)) {
    throw new Error("LLM transcript correction response must contain a turns array.");
  }
  return parsed.turns as CorrectionResponseTurn[];
}

function batchTurns(turns: TranscriptTurn[], maxChars: number): TranscriptTurn[][] {
  const batches: TranscriptTurn[][] = [];
  let currentBatch: TranscriptTurn[] = [];
  let currentChars = 0;

  for (const turn of turns) {
    const turnChars = JSON.stringify(toCorrectionInputTurn(turn)).length;
    if (currentBatch.length > 0 && currentChars + turnChars > maxChars) {
      batches.push(currentBatch);
      currentBatch = [];
      currentChars = 0;
    }

    currentBatch.push(turn);
    currentChars += turnChars;
  }

  if (currentBatch.length > 0) {
    batches.push(currentBatch);
  }

  return batches;
}

function buildCorrectionSystemPrompt(language: string): string {
  return [
    "You conservatively correct ASR transcript typos for EchoNote.",
    "Return only valid JSON. Do not wrap it in markdown.",
    "Only correct obvious typos, homophone mistakes, punctuation, product-name spacing, and capitalization.",
    "Do not summarize, rewrite meaning, add facts, remove content, merge turns, split turns, reorder turns, or change speakers.",
    "The output JSON shape must be: {\"turns\":[{\"id\":\"...\",\"text\":\"...\"}]}",
    "The output turns array must have the same length and id order as the input.",
    `Transcript language: ${language}.`
  ].join("\n");
}

function buildCorrectionUserPrompt(turns: TranscriptTurn[]): string {
  return JSON.stringify(
    {
      turns: turns.map(toCorrectionInputTurn)
    },
    null,
    2
  );
}

function toCorrectionInputTurn(turn: TranscriptTurn): CorrectionInputTurn {
  return {
    id: turn.id,
    speaker: turn.speaker,
    started_at_ms: turn.started_at_ms,
    ended_at_ms: turn.ended_at_ms,
    text: turn.text
  };
}

function validateCorrectedBatch(
  originalTurns: TranscriptTurn[],
  correctedTurns: CorrectionResponseTurn[],
  correctionRules: string
): { turns: TranscriptTurn[]; acceptedTurns: number; changedTurns: number; rejectedTurns: number } {
  if (correctedTurns.length !== originalTurns.length) {
    throw new Error("LLM transcript correction returned a mismatched turn count.");
  }

  const turns: TranscriptTurn[] = [];
  let acceptedTurns = 0;
  let changedTurns = 0;
  let rejectedTurns = 0;

  for (let index = 0; index < originalTurns.length; index += 1) {
    const original = originalTurns[index];
    const corrected = correctedTurns[index];
    if (corrected.id !== original.id || typeof corrected.text !== "string") {
      turns.push(original);
      rejectedTurns += 1;
      continue;
    }

    const correctedText = applyTranscriptCorrections(sanitizeTranscriptText(corrected.text), correctionRules);
    if (!isSafeCorrection(original.text, correctedText)) {
      turns.push(original);
      rejectedTurns += 1;
      continue;
    }

    turns.push({
      ...original,
      text: correctedText
    });
    acceptedTurns += 1;
    if (correctedText !== original.text) {
      changedTurns += 1;
    }
  }

  return { turns, acceptedTurns, changedTurns, rejectedTurns };
}

function isSafeCorrection(originalText: string, correctedText: string): boolean {
  if (originalText && !correctedText) {
    return false;
  }

  if (originalText.length < MIN_RATIO_CHECK_CHARS) {
    return correctedText.length <= Math.max(40, originalText.length * 3);
  }

  const ratio = correctedText.length / originalText.length;
  return ratio >= MIN_LENGTH_RATIO && ratio <= MAX_LENGTH_RATIO;
}

function extractJson(rawText: string): string {
  const trimmed = rawText.trim();
  const fenced = trimmed.match(/```(?:json)?\s*([\s\S]*?)\s*```/i);
  if (fenced) {
    return fenced[1].trim();
  }

  const firstBrace = trimmed.indexOf("{");
  const lastBrace = trimmed.lastIndexOf("}");
  if (firstBrace >= 0 && lastBrace > firstBrace) {
    return trimmed.slice(firstBrace, lastBrace + 1);
  }

  return trimmed;
}
