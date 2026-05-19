import { App, TFile, normalizePath } from "obsidian";
import type { TranscriptSegment } from "../asr/asr-types";
import type { EchoNoteSettings } from "../settings/settings";
import { formatMeetingTitle, formatTranscriptTimestamp, sanitizeFileName } from "../utils/time";
import type { MeetingSummary } from "../llm/llm-types";
import { extractTranscript, replaceSummarySections } from "./markdown-sections";
import { renderMeetingTemplate } from "./meeting-template";

type CreateMeetingOptions = {
  settings: EchoNoteSettings;
  startTime: Date;
  asrModel: string;
};

export type MeetingNoteInfo = {
  file: TFile;
  title: string;
  audioFolder: string;
};

export class MeetingNoteWriter {
  constructor(private readonly app: App) {}

  async createMeetingNote(options: CreateMeetingOptions): Promise<MeetingNoteInfo> {
    const title = sanitizeFileName(formatMeetingTitle(options.settings.meetingTitleFormat, options.startTime));
    const folder = normalizePath(options.settings.meetingFolder || "Meetings");
    await this.ensureFolder(folder);

    const path = await this.nextAvailablePath(folder, `${title}.md`);
    const content = renderMeetingTemplate(options.settings, {
      title,
      startTime: options.startTime,
      asrModel: options.asrModel,
      llmProvider: options.settings.llmProvider
    });

    const file = await this.app.vault.create(path, content);
    return {
      file,
      title,
      audioFolder: normalizePath(`${options.settings.audioSaveFolder || "Meetings/audio"}/${title}`)
    };
  }

  async appendTranscript(file: TFile, segment: TranscriptSegment, enableTimestamps: boolean): Promise<void> {
    const timestamp = enableTimestamps ? `[${formatTranscriptTimestamp(segment.started_at_ms)}] ` : "";
    const text = segment.text.trim();
    if (!text) {
      return;
    }

    await this.app.vault.append(file, `\n${timestamp}${text}\n`);
  }

  async saveMeetingAudio(folder: string, meetingTitle: string, wavBytes: ArrayBuffer): Promise<void> {
    const normalizedFolder = normalizePath(folder || `Meetings/audio/${meetingTitle}`);
    await this.ensureFolder(normalizedFolder);
    const safeTitle = sanitizeFileName(meetingTitle);
    const path = await this.nextAvailablePath(normalizedFolder, `${safeTitle}.wav`);
    await this.app.vault.createBinary(path, wavBytes);
  }

  async readTranscript(file: TFile): Promise<string> {
    const content = await this.app.vault.read(file);
    return extractTranscript(content);
  }

  async writeSummary(file: TFile, summary: MeetingSummary): Promise<void> {
    const content = await this.app.vault.read(file);
    const updated = replaceSummarySections(content, {
      Summary: summary.summary,
      Decisions: formatBullets(summary.decisions),
      "Action Items": formatTasks(summary.actionItems),
      "Key Points": formatBullets(summary.keyPoints),
      "Open Questions": formatBullets(summary.openQuestions)
    });
    await this.app.vault.modify(file, updated);
  }

  private async ensureFolder(path: string): Promise<void> {
    if (!path || this.app.vault.getAbstractFileByPath(path)) {
      return;
    }

    const parts = path.split("/");
    let current = "";
    for (const part of parts) {
      current = current ? `${current}/${part}` : part;
      if (!this.app.vault.getAbstractFileByPath(current)) {
        await this.app.vault.createFolder(current);
      }
    }
  }

  private async nextAvailablePath(folder: string, fileName: string): Promise<string> {
    const baseName = fileName.replace(/\.md$|\.wav$/i, "");
    const extension = fileName.endsWith(".wav") ? ".wav" : ".md";
    let candidate = normalizePath(`${folder}/${fileName}`);
    let index = 2;

    while (this.app.vault.getAbstractFileByPath(candidate)) {
      candidate = normalizePath(`${folder}/${baseName} ${index}${extension}`);
      index += 1;
    }

    return candidate;
  }
}

function formatBullets(items: string[]): string {
  const normalized = items.map((item) => item.trim()).filter(Boolean);
  if (normalized.length === 0) {
    return "_None._";
  }
  return normalized.map((item) => `- ${item}`).join("\n");
}

function formatTasks(items: string[]): string {
  const normalized = items.map((item) => item.trim()).filter(Boolean);
  if (normalized.length === 0) {
    return "_None._";
  }
  return normalized.map((item) => `- [ ] ${item}`).join("\n");
}
