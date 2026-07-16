import type { MeetingSummary } from "./llm-types";

export function buildSummarySystemPrompt(language: string): string {
  return [
    "You summarize meeting transcripts for EchoNote.",
    "Return only valid JSON. Do not wrap it in markdown.",
    "The JSON shape must be:",
    '{"meetingTitle":"...","summary":"...","decisions":["..."],"actionItems":["..."],"keyPoints":["..."],"openQuestions":["..."]}',
    "meetingTitle must be a concise meeting topic only, without a date or filename punctuation.",
    `Summary language: ${language}.`
  ].join("\n");
}

export function buildSummaryUserPrompt(transcript: string, customPrompt: string): string {
  return [
    customPrompt.trim(),
    "",
    "Transcript:",
    transcript.trim()
  ].join("\n");
}

export function parseMeetingSummary(rawText: string): MeetingSummary {
  const jsonText = extractJson(rawText);
  const parsed = JSON.parse(jsonText) as Partial<MeetingSummary>;

  return {
    meetingTitle: normalizeString(parsed.meetingTitle),
    summary: normalizeString(parsed.summary),
    decisions: normalizeStringArray(parsed.decisions),
    actionItems: normalizeStringArray(parsed.actionItems),
    keyPoints: normalizeStringArray(parsed.keyPoints),
    openQuestions: normalizeStringArray(parsed.openQuestions)
  };
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

function normalizeString(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function normalizeStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value.map((item) => (typeof item === "string" ? item.trim() : "")).filter(Boolean);
}
