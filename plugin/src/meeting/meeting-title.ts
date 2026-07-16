import { formatDate, sanitizeFileName } from "../utils/time";

const MAX_MEETING_TOPIC_LENGTH = 80;

export function createSummarizedMeetingTitle(
  markdown: string,
  currentTitle: string,
  meetingTopic: string,
  summary: string,
  createdAt: Date
): string {
  const date = extractMeetingDate(markdown) ?? extractMeetingDate(currentTitle) ?? formatDate(createdAt);
  const topic = normalizeMeetingTopic(meetingTopic || summary) || "Meeting";
  return sanitizeFileName(`${date}_${topic}`);
}

export function extractMeetingDate(value: string): string | null {
  const match = value.match(/(?:^|\n)\s*(?:-\s*)?Date:\s*(\d{4}-\d{2}-\d{2})\b/i)
    ?? value.match(/\b(\d{4}-\d{2}-\d{2})\b/);
  return match?.[1] ?? null;
}

export function normalizeMeetingTopic(value: string): string {
  const firstSentence = value.trim().split(/[\n。！？!?]/, 1)[0] ?? "";
  return sanitizeFileName(firstSentence)
    .replace(/^\d{4}[-/.年]\d{1,2}[-/.月]\d{1,2}日?[\s_-]*/, "")
    .replace(/_/g, " ")
    .replace(/\s+/g, " ")
    .replace(/^[.\s-]+|[.\s-]+$/g, "")
    .slice(0, MAX_MEETING_TOPIC_LENGTH)
    .trim();
}
