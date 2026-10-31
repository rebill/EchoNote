// Payloads of the Rust commands and events (app/src-tauri/src/views.rs, commands.rs).

export type Track = "mic" | "sys";
export type SessionState = "starting" | "recording" | "paused" | "stopping" | "finalized";
export type MeetingStatus = "recording" | "finalized" | "recovered";

export interface Segment {
  id: number;
  track: Track;
  startMs: number;
  endMs: number;
  text: string;
  crosstalk: "trim" | "duplicate" | null;
  source: "asr" | "llm" | "user";
  revision: number;
}

export type Notice =
  | { kind: "modelLoading" }
  | { kind: "modelLoaded"; seconds: number }
  | { kind: "modelFailed"; error: string }
  | { kind: "sourceStarted"; track: Track; name: string }
  | { kind: "sourceFailed"; track: Track; error: string }
  | { kind: "permissionDenied"; track: Track }
  | { kind: "deviceChanged"; track: Track }
  | { kind: "bleed" }
  | { kind: "autoStopped" }
  | { kind: "storeFailed"; error: string }
  | { kind: "writeFailed"; track: Track; error: string };

export interface Finished {
  durationMs: number;
  finals: [number, number];
  hidden: number;
  trimmed: number;
  droppedFrames: number;
  gapMs: [number, number];
  spilledS: number;
  decodeS: number;
}

export interface Delta {
  meeting: number;
  state: SessionState | null;
  elapsedMs: number;
  levels: [number, number] | null;
  previews: [string, string] | null;
  segments: Segment[];
  notices: Notice[] | null;
  finished: Finished | null;
}

export interface Snapshot {
  meeting: number;
  state: SessionState;
  elapsedMs: number;
  previews: [string, string];
  notices: Notice[];
  segments: Segment[];
}

export interface Meeting {
  id: number;
  title: string;
  startedAtMs: number;
  endedAtMs: number | null;
  durationMs: number | null;
  status: MeetingStatus;
  language: string | null;
}

export interface MeetingDetail {
  meeting: Meeting;
  tracks: { kind: Track; deviceName: string | null; durationMs: number | null }[];
  segments: Segment[];
}

export interface SearchHit {
  meetingId: number;
  meetingTitle: string;
  segmentId: number;
  track: Track;
  startMs: number;
  text: string;
}

export interface CommandError {
  code: string;
  message: string;
}

export interface AppInfo {
  version: string;
  platform: "macos" | "windows" | "other";
  dataDir: string;
  modelInstalled: boolean;
  vadInstalled: boolean;
  engineError: CommandError | null;
  shortcutError: string | null;
  pendingError: CommandError | null;
  smoke: boolean;
}

export type Language = "zh-CN" | "en";

export interface Settings {
  language: Language;
  micDevice: string | null;
  recordSystem: boolean;
  autoStopMinutes: number;
  shortcut: string;
}

export interface Device {
  id: string;
  name: string;
  isDefault: boolean;
}

export type Permission = "granted" | "denied" | "undetermined";

export interface Permissions {
  mic: Permission;
  sys: Permission;
}

/// One line in a transcript list: a final, or the utterance in progress (preview).
export interface TranscriptRow {
  key: string;
  segmentId: number | null;
  track: Track;
  startMs: number;
  text: string;
  preview: boolean;
  /// Speaker bleed shown on request.
  hidden: boolean;
  /// The text is not the recognized one (edited by the user or the LLM).
  edited: boolean;
  /// Words were cut as speaker bleed and can be put back.
  trimmed: boolean;
}

export interface GlossaryEntry {
  id: number;
  term: string;
  aliases: string[];
}
