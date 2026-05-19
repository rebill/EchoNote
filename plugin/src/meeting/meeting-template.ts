import type { EchoNoteSettings } from "../settings/settings";
import { formatClockTime, formatDate } from "../utils/time";

type MeetingTemplateContext = {
  title: string;
  startTime: Date;
  endTime?: Date | null;
  asrModel: string;
  llmProvider: string;
};

export function renderMeetingTemplate(settings: EchoNoteSettings, context: MeetingTemplateContext): string {
  const endTime = context.endTime ? formatClockTime(context.endTime) : "";

  return settings.meetingTemplate
    .replaceAll("{{meeting_title}}", context.title)
    .replaceAll("{{date}}", formatDate(context.startTime))
    .replaceAll("{{start_time}}", formatClockTime(context.startTime))
    .replaceAll("{{end_time}}", endTime)
    .replaceAll("{{asr_model}}", context.asrModel)
    .replaceAll("{{llm_provider}}", context.llmProvider);
}
