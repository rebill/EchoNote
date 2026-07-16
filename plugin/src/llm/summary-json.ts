import type { MeetingSummary } from "./llm-types";

export class MeetingSummaryParseError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "MeetingSummaryParseError";
  }
}

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
  let parsed: unknown;
  try {
    parsed = JSON.parse(jsonText) as unknown;
  } catch (error) {
    throw new MeetingSummaryParseError(
      `Meeting summary was not valid JSON: ${error instanceof Error ? error.message : String(error)}`
    );
  }

  if (!isRecord(parsed)) {
    throw new MeetingSummaryParseError("Meeting summary JSON must be an object.");
  }

  return {
    meetingTitle: requireNonEmptyString(parsed.meetingTitle, "meetingTitle"),
    summary: requireNonEmptyString(parsed.summary, "summary"),
    decisions: requireStringArray(parsed.decisions, "decisions"),
    actionItems: requireStringArray(parsed.actionItems, "actionItems"),
    keyPoints: requireStringArray(parsed.keyPoints, "keyPoints"),
    openQuestions: requireStringArray(parsed.openQuestions, "openQuestions")
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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requireNonEmptyString(value: unknown, field: string): string {
  if (typeof value !== "string" || !value.trim()) {
    throw new MeetingSummaryParseError(`Meeting summary field ${field} must be a non-empty string.`);
  }
  return value.trim();
}

function requireStringArray(value: unknown, field: string): string[] {
  if (!Array.isArray(value) || !value.every((item): item is string => typeof item === "string")) {
    throw new MeetingSummaryParseError(`Meeting summary field ${field} must be an array of strings.`);
  }

  return value.map((item) => item.trim()).filter(Boolean);
}
