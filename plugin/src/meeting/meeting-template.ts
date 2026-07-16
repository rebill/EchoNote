import type { EchoNoteSettings } from "../settings/settings";
import { formatClockTime, formatDate } from "../utils/time";

export const ECHONOTE_MEETING_MARKER = "<!-- echonote-meeting -->";

type MeetingTemplateContext = {
  title: string;
  startTime: Date;
  endTime?: Date | null;
  asrModel: string;
  llmProvider: string;
};

export function renderMeetingTemplate(settings: EchoNoteSettings, context: MeetingTemplateContext): string {
  const endTime = context.endTime ? formatClockTime(context.endTime) : "";

  const rendered = settings.meetingTemplate
    .replaceAll("{{meeting_title}}", context.title)
    .replaceAll("{{date}}", formatDate(context.startTime))
    .replaceAll("{{start_time}}", formatClockTime(context.startTime))
    .replaceAll("{{end_time}}", endTime)
    .replaceAll("{{asr_model}}", context.asrModel)
    .replaceAll("{{llm_provider}}", context.llmProvider);

  if (rendered.includes(ECHONOTE_MEETING_MARKER)) {
    return rendered;
  }

  const frontmatter = rendered.match(/^---\r?\n[\s\S]*?\r?\n---(?:\r?\n|$)/);
  if (frontmatter) {
    return `${frontmatter[0]}${ECHONOTE_MEETING_MARKER}\n${rendered.slice(frontmatter[0].length)}`;
  }

  return `${ECHONOTE_MEETING_MARKER}\n${rendered}`;
}
