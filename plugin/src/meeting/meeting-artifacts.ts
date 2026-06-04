import type { EchoNoteSettings } from "../settings/settings";
import { sanitizeFileName } from "../utils/time";

type AudioFolderSettings = Pick<EchoNoteSettings, "audioSaveFolder">;

export type MeetingArtifactPaths = {
  folder: string;
  audioPath: string;
  segmentsPath: string;
};

export function getMeetingArtifactPaths(settings: AudioFolderSettings, meetingTitle: string): MeetingArtifactPaths {
  const folder = getMeetingAudioFolder(settings, meetingTitle);
  const baseName = getMeetingArtifactBaseName(meetingTitle);
  const audioPath = normalizeVaultPath(`${folder}/${baseName}.wav`);
  return {
    folder,
    audioPath,
    segmentsPath: getSegmentsPathForAudioPath(audioPath)
  };
}

export function getMeetingAudioFolder(settings: AudioFolderSettings, meetingTitle: string): string {
  const audioSaveFolder = settings.audioSaveFolder || "Meetings/audio";
  return normalizeVaultPath(`${audioSaveFolder}/${getMeetingArtifactBaseName(meetingTitle)}`);
}

export function getMeetingArtifactBaseName(meetingTitle: string): string {
  return sanitizeFileName(meetingTitle || "meeting") || "meeting";
}

export function getSegmentsPathForAudioPath(audioPath: string): string {
  const normalizedPath = normalizeVaultPath(audioPath);
  if (/\.wav$/i.test(normalizedPath)) {
    return normalizedPath.replace(/\.wav$/i, ".segments.json");
  }
  return `${normalizedPath}.segments.json`;
}

export function normalizeVaultPath(path: string): string {
  return path
    .trim()
    .replace(/\\/g, "/")
    .replace(/\/{2,}/g, "/")
    .replace(/^\/+/, "")
    .replace(/\/+$/, "");
}

export function sanitizeMeetingId(value: string): string {
  return value.replace(/[^a-zA-Z0-9_-]+/g, "-").replace(/^-+|-+$/g, "") || "meeting";
}
