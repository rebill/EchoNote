import { App, TFile, normalizePath } from "obsidian";
import type { TranscriptSegment, TranscriptTurn } from "../asr/asr-types";
import type { EchoNoteSettings } from "../settings/settings";
import { formatClockTime, formatMeetingTitle, formatTranscriptTimestamp, pad2, sanitizeFileName } from "../utils/time";
import type { MeetingSummary } from "../llm/llm-types";
import {
  extractTranscript,
  replaceDocumentTitle,
  replaceMeetingEndTime,
  replaceSummarySections,
  replaceTranscriptSection
} from "./markdown-sections";
import { renderMeetingTemplate } from "./meeting-template";
import { createSummarizedMeetingTitle } from "./meeting-title";
import { applyTranscriptCorrections } from "./transcript-corrections";
import { sanitizeTranscriptText } from "./transcript-sanitizer";
import { formatTranscriptTurns, parseTranscriptMarkdown, type ParsedTranscript } from "./transcript-markdown";
import {
  getMeetingArtifactBaseName,
  getMeetingArtifactPaths,
  getMeetingAudioFolder,
  getSegmentsPathForAudioPath,
  normalizeVaultPath
} from "./meeting-artifacts";

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
      audioFolder: getMeetingAudioFolder(options.settings, title)
    };
  }

  async appendTranscript(
    file: TFile,
    segment: TranscriptSegment,
    enableTimestamps: boolean,
    correctionRules: string
  ): Promise<void> {
    const timestamp = enableTimestamps ? `[${formatTranscriptTimestamp(segment.started_at_ms)}] ` : "";
    const text = applyTranscriptCorrections(sanitizeTranscriptText(segment.text), correctionRules);
    if (!text) {
      return;
    }

    await this.app.vault.append(file, `\n${timestamp}${text}\n`);
  }

  async replaceTranscript(
    file: TFile,
    turns: TranscriptTurn[],
    enableTimestamps: boolean,
    correctionRules: string
  ): Promise<void> {
    const rendered = formatTranscriptTurns(turns, enableTimestamps, correctionRules);
    if (!rendered) {
      return;
    }

    const content = await this.app.vault.read(file);
    await this.app.vault.modify(file, replaceTranscriptSection(content, rendered));
  }

  async updateMeetingEndTime(file: TFile, endTime: Date): Promise<void> {
    const content = await this.app.vault.read(file);
    const updated = replaceMeetingEndTime(content, formatClockTime(endTime));
    if (updated !== content) {
      await this.app.vault.modify(file, updated);
    }
  }

  async saveMeetingAudio(folder: string, meetingTitle: string, wavBytes: ArrayBuffer): Promise<string> {
    const normalizedFolder = normalizePath(folder || `Meetings/audio/${meetingTitle}`);
    await this.ensureFolder(normalizedFolder);
    const safeTitle = getMeetingArtifactBaseName(meetingTitle);
    const path = await this.nextAvailablePath(normalizedFolder, `${safeTitle}.wav`);
    await this.app.vault.createBinary(path, wavBytes);
    return path;
  }

  async saveMeetingSegments(audioPath: string, segments: TranscriptSegment[]): Promise<string> {
    const path = getSegmentsPathForAudioPath(audioPath);
    await this.createOrOverwriteText(path, JSON.stringify(segments, null, 2));
    return path;
  }

  async readMeetingAudio(path: string): Promise<ArrayBuffer> {
    const file = this.getVaultFile(path);
    if (!file) {
      throw new Error(`Saved meeting audio was not found: ${path}`);
    }
    return this.app.vault.readBinary(file);
  }

  async readMeetingSegments(path: string): Promise<TranscriptSegment[]> {
    const file = this.getVaultFile(path);
    if (!file) {
      throw new Error(`Saved transcript segments were not found: ${path}`);
    }

    const content = await this.app.vault.read(file);
    const parsed = JSON.parse(content) as unknown;
    if (!Array.isArray(parsed)) {
      throw new Error(`Saved transcript segments must be a JSON array: ${path}`);
    }
    return parsed as TranscriptSegment[];
  }

  async readTranscript(file: TFile): Promise<string> {
    const content = await this.app.vault.read(file);
    return extractTranscript(content);
  }

  async readParsedTranscript(file: TFile): Promise<ParsedTranscript> {
    return parseTranscriptMarkdown(await this.readTranscript(file));
  }

  async saveTranscriptBeforeLlmArtifact(file: TFile, transcript: string, now = new Date()): Promise<string> {
    const artifactFolder = getTranscriptCorrectionArtifactFolder(file.path);
    await this.ensureFolder(artifactFolder);
    const fileName = `${sanitizeFileName(file.basename) || "meeting"}.transcript.before-llm.${formatArtifactTimestamp(now)}.md`;
    const path = await this.nextAvailableAdapterPath(artifactFolder, fileName);
    await this.app.vault.adapter.write(path, `${transcript.trim()}\n`);
    return path;
  }

  async writeTranscriptCorrectionMetadata(
    file: TFile,
    correctedAt = new Date(),
    changedTurns = 0,
    totalTurns = 0
  ): Promise<void> {
    const content = await this.app.vault.read(file);
    const updated = upsertTranscriptCorrectionMetadata(
      content,
      formatMetadataTimestamp(correctedAt),
      changedTurns,
      totalTurns
    );
    if (updated !== content) {
      await this.app.vault.modify(file, updated);
    }
  }

  async writeSummary(file: TFile, summary: MeetingSummary, settings: EchoNoteSettings): Promise<TFile> {
    const previousTitle = file.basename;
    const content = await this.app.vault.read(file);
    const title = createSummarizedMeetingTitle(
      content,
      file.basename,
      summary.meetingTitle,
      summary.summary,
      new Date(file.stat.ctime)
    );
    const folder = file.parent?.path ?? "";
    const targetPath = await this.nextAvailablePath(folder, `${title}.md`, file.path);
    const resolvedTitle = targetPath.split("/").pop()?.replace(/\.md$/i, "") ?? title;
    const summarized = replaceSummarySections(content, {
      Summary: summary.summary,
      Decisions: formatBullets(summary.decisions),
      "Action Items": formatTasks(summary.actionItems),
      "Key Points": formatBullets(summary.keyPoints),
      "Open Questions": formatBullets(summary.openQuestions)
    });
    const updated = replaceDocumentTitle(summarized, resolvedTitle);
    await this.app.vault.modify(file, updated);
    const renamedFile = await this.renameMeetingNote(file, targetPath);
    try {
      await this.renameMeetingArtifacts(settings, previousTitle, renamedFile.basename);
    } catch (error) {
      console.warn("EchoNote could not rename saved meeting artifacts with the summarized note.", error);
    }
    return renamedFile;
  }

  private async renameMeetingNote(file: TFile, path: string): Promise<TFile> {
    if (path === file.path) {
      return file;
    }

    await this.app.fileManager.renameFile(file, path);
    return this.getVaultFile(path) ?? file;
  }

  private async renameMeetingArtifacts(
    settings: EchoNoteSettings,
    previousTitle: string,
    renamedTitle: string
  ): Promise<void> {
    if (previousTitle === renamedTitle) {
      return;
    }

    const previousFolder = getMeetingAudioFolder(settings, previousTitle);
    const renamedFolder = getMeetingAudioFolder(settings, renamedTitle);
    const artifactFolder = this.app.vault.getAbstractFileByPath(previousFolder);
    if (!artifactFolder) {
      return;
    }
    if (this.app.vault.getAbstractFileByPath(renamedFolder)) {
      throw new Error(`Meeting artifact folder already exists: ${renamedFolder}`);
    }

    const previousPaths = getMeetingArtifactPaths(settings, previousTitle);
    const renamedPaths = getMeetingArtifactPaths(settings, renamedTitle);
    await this.app.fileManager.renameFile(artifactFolder, renamedFolder);

    const movedAudioPath = normalizePath(`${renamedFolder}/${previousPaths.audioPath.split("/").pop() ?? ""}`);
    const movedSegmentsPath = normalizePath(`${renamedFolder}/${previousPaths.segmentsPath.split("/").pop() ?? ""}`);
    await this.renameArtifactFile(movedAudioPath, renamedPaths.audioPath);
    await this.renameArtifactFile(movedSegmentsPath, renamedPaths.segmentsPath);
  }

  private async renameArtifactFile(currentPath: string, targetPath: string): Promise<void> {
    if (currentPath === targetPath) {
      return;
    }

    const file = this.getVaultFile(currentPath);
    if (!file) {
      return;
    }
    if (this.app.vault.getAbstractFileByPath(targetPath)) {
      throw new Error(`Meeting artifact already exists: ${targetPath}`);
    }

    await this.app.fileManager.renameFile(file, targetPath);
  }

  private async ensureFolder(path: string): Promise<void> {
    const normalizedPath = normalizePath(path);
    if (!normalizedPath || await this.vaultPathExists(normalizedPath)) {
      return;
    }

    const parts = normalizedPath.split("/");
    let current = "";
    for (const part of parts) {
      current = current ? `${current}/${part}` : part;
      if (!await this.vaultPathExists(current)) {
        await this.createFolderIfMissing(current);
      }
    }
  }

  private async createFolderIfMissing(path: string): Promise<void> {
    try {
      await this.app.vault.createFolder(path);
    } catch (error) {
      if (!isAlreadyExistsError(error) && !await this.vaultPathExists(path)) {
        throw error;
      }
    }
  }

  private async vaultPathExists(path: string): Promise<boolean> {
    if (this.app.vault.getAbstractFileByPath(path)) {
      return true;
    }

    return this.app.vault.adapter.exists(path);
  }

  private async nextAvailableAdapterPath(folder: string, fileName: string): Promise<string> {
    const baseName = fileName.replace(/\.md$|\.wav$/i, "");
    const extension = fileName.endsWith(".wav") ? ".wav" : ".md";
    let candidate = normalizePath(`${folder}/${fileName}`);
    let index = 2;

    while (await this.app.vault.adapter.exists(candidate)) {
      candidate = normalizePath(`${folder}/${baseName} ${index}${extension}`);
      index += 1;
    }

    return candidate;
  }

  private async createOrOverwriteText(path: string, content: string): Promise<void> {
    const normalizedPath = normalizeVaultPath(path);
    const folder = normalizedPath.split("/").slice(0, -1).join("/");
    if (folder) {
      await this.ensureFolder(folder);
    }

    const existingFile = this.getVaultFile(normalizedPath);
    if (existingFile) {
      await this.app.vault.modify(existingFile, content);
      return;
    }

    await this.app.vault.create(normalizedPath, content);
  }

  private getVaultFile(path: string): TFile | null {
    const file = this.app.vault.getAbstractFileByPath(normalizePath(path));
    return file instanceof TFile ? file : null;
  }

  private async nextAvailablePath(folder: string, fileName: string, ignoredPath?: string): Promise<string> {
    const baseName = fileName.replace(/\.md$|\.wav$/i, "");
    const extension = fileName.endsWith(".wav") ? ".wav" : ".md";
    let candidate = normalizePath(`${folder}/${fileName}`);
    let index = 2;

    while (candidate !== ignoredPath && this.app.vault.getAbstractFileByPath(candidate)) {
      candidate = normalizePath(`${folder}/${baseName} ${index}${extension}`);
      index += 1;
    }

    return candidate;
  }
}

function isAlreadyExistsError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return /already exists/i.test(message);
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

function getTranscriptCorrectionArtifactFolder(filePath: string): string {
  const folder = normalizePath(filePath).split("/").slice(0, -1).join("/");
  return normalizePath(folder ? `${folder}/.echonote-artifacts` : ".echonote-artifacts");
}

function upsertTranscriptCorrectionMetadata(
  markdown: string,
  correctedAt: string,
  changedTurns: number,
  totalTurns: number
): string {
  const line = `- Transcript Correction: LLM checked at ${correctedAt}; changed ${changedTurns} of ${totalTurns} turn(s)`;
  const existing = /^-[ \t]*Transcript Correction:[^\n]*$/m;
  if (existing.test(markdown)) {
    return markdown.replace(existing, line);
  }

  const firstSectionIndex = markdown.search(/^##\s+/m);
  if (firstSectionIndex < 0) {
    return `${markdown.trimEnd()}\n${line}\n`;
  }

  const beforeSections = markdown.slice(0, firstSectionIndex).trimEnd();
  const sections = markdown.slice(firstSectionIndex).trimStart();
  if (!beforeSections) {
    return `${line}\n\n${sections}`;
  }

  return `${beforeSections}\n${line}\n\n${sections}`;
}

function formatArtifactTimestamp(date: Date): string {
  return [
    date.getFullYear(),
    pad2(date.getMonth() + 1),
    pad2(date.getDate()),
    "-",
    pad2(date.getHours()),
    pad2(date.getMinutes()),
    pad2(date.getSeconds())
  ].join("");
}

function formatMetadataTimestamp(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
}
