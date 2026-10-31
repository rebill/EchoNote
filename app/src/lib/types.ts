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
  audioBytes: number;
  notes: SummaryView | null;
  highlights: SummaryView | null;
  qa: QaTurn[];
  /// LLM changes waiting for review, with the text before them.
  corrections: { segmentId: number; previousText: string }[];
  /// Segments the high-accuracy model changed that undo would restore.
  retranscribed: number;
}

export interface SummaryView {
  id: number;
  template: string;
  /// `name / model`.
  provider: string;
  title: string | null;
  content: string;
  truncated: boolean;
  createdAtMs: number;
}

export interface QaTurn {
  id: number;
  question: string;
  answer: string;
  provider: string;
  createdAtMs: number;
  truncated: boolean;
  excerpt: boolean;
}

export type Api = "openAi" | "anthropic";

export interface Provider {
  id: string;
  name: string;
  preset: string;
  api: Api;
  baseUrl: string;
  model: string;
  contextTokens: number;
  maxOutputTokens: number;
  proxy: string | null;
  allowHttp: boolean;
  headerNames: string[];
  hasKey: boolean;
  consentedOrigin: string | null;
  /// Runs on this computer: nothing leaves it.
  local: boolean;
  origin: string | null;
  needsConsent: boolean;
}

/// What the provider form sends. `apiKey` null keeps the saved key, "" removes it; an empty
/// header value keeps that header's saved value.
export interface ProviderInput {
  id: string | null;
  name: string;
  preset: string;
  api: Api;
  baseUrl: string;
  model: string;
  contextTokens: number;
  maxOutputTokens: number;
  proxy: string | null;
  allowHttp: boolean;
  apiKey: string | null;
  headers: [string, string][];
}

export interface Template {
  id: number;
  name: string;
  instructions: string;
}

export interface AiSettings {
  providers: Provider[];
  active: string | null;
  /// A built-in id or `custom:<id>`.
  template: string;
  liveHighlights: boolean;
  builtinTemplates: string[];
  templates: Template[];
}

export type AiKind = "notes" | "answer" | "correct" | "highlights" | "retranscribe";

export type Stage =
  | { stage: "parts"; done: number; total: number }
  | { stage: "merging"; notes: number }
  | { stage: "writing" }
  | { stage: "batches"; done: number; total: number }
  | { stage: "segments"; done: number; total: number };

export interface RetranscribeOutcome {
  done: number;
  total: number;
  changed: number;
}

export type ModelRole = "live" | "vad" | "accurate";

/// A model in the built-in manifest (`asr_models`).
export interface ModelInfo {
  id: string;
  role: ModelRole;
  size: number;
  memoryMb: number;
  license: string;
  installed: boolean;
  /// Downloaded into the data directory (delete can remove it).
  removable: boolean;
  /// Bytes kept from an unfinished download.
  partial: number;
  downloading: boolean;
}

export interface ModelProgress {
  stage: "downloading" | "extracting";
  done: number;
  total: number;
}

/// `models` event.
export type ModelEvent =
  | ({ event: "progress"; id: string } & ModelProgress)
  | { event: "done"; id: string }
  | { event: "error"; id: string; error: CommandError };

export interface CorrectOutcome {
  applied: number;
  rejected: number;
  failedBatches: number;
  batches: number;
}

/// `ai` event; job 0 is live highlights.
export type AiEvent = { job: number; meeting: number; kind: AiKind } & (
  | { event: "text"; text: string }
  | ({ event: "progress" } & Stage)
  | { event: "done"; result: unknown }
  | { event: "error"; error: CommandError }
);

export interface StorageUsage {
  libraryBytes: number;
  audioBytes: number;
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
  smokeLlm: string | null;
  smokeMirror: boolean;
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
