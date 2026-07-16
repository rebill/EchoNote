export type LlmProviderType = "openai-compatible" | "anthropic";

export type ChunkLengthSeconds = 10 | 15 | 30;
export type SummaryLanguage = "auto" | "zh" | "en";
export type AsrRuntimeMode = "companion";

export const DEFAULT_COMPANION_DISCOVERY_PATH = "~/Library/Application Support/EchoNote/companion.json";
export const DEFAULT_COMPANION_DISCOVERY_MAX_AGE_SECONDS = 30;
export const DEFAULT_AUTO_STOP_SILENCE_MINUTES = 10;
export const MIN_AUTO_STOP_SILENCE_MINUTES = 1;
export const MAX_AUTO_STOP_SILENCE_MINUTES = 60;

export type EchoNoteSettings = {
  meetingFolder: string;
  meetingTitleFormat: string;
  meetingTemplate: string;
  enableTimestamps: boolean;

  asrRuntimeMode: AsrRuntimeMode;
  companionDiscoveryPath: string;
  companionDiscoveryMaxAgeSeconds: number;
  chunkLengthSeconds: ChunkLengthSeconds;
  transcriptCorrectionRules: string;
  enableLlmTranscriptCorrection: boolean;

  audioInputDeviceId: string;
  audioInputDeviceLabel: string;
  autoStopOnSilence: boolean;
  autoStopSilenceMinutes: number;
  saveRawAudio: boolean;
  audioSaveFolder: string;

  llmProvider: LlmProviderType;
  openaiApiKey: string;
  openaiBaseUrl: string;
  openaiModel: string;
  anthropicApiKey: string;
  anthropicModel: string;
  summaryLanguage: SummaryLanguage;
  summaryPrompt: string;
};

export const DEFAULT_MEETING_TEMPLATE = `# {{meeting_title}}

- Date: {{date}}
- Time: {{start_time}} - {{end_time}}
- Platform: EchoNote
- ASR Model: {{asr_model}}
- LLM Provider: {{llm_provider}}
- Tags: #meeting #echonote

## Summary

_Pending._

## Decisions

_Pending._

## Action Items

_Pending._

## Key Points

_Pending._

## Open Questions

_Pending._

## Transcript
`;

export const DEFAULT_SUMMARY_PROMPT = `You are EchoNote's meeting summarizer. Return structured JSON with meetingTitle, summary, decisions, actionItems, keyPoints, and openQuestions.`;

export const DEFAULT_SETTINGS: EchoNoteSettings = {
  meetingFolder: "Meetings",
  meetingTitleFormat: "YYYY-MM-DD HH-mm Meeting",
  meetingTemplate: DEFAULT_MEETING_TEMPLATE,
  enableTimestamps: true,

  asrRuntimeMode: "companion",
  companionDiscoveryPath: DEFAULT_COMPANION_DISCOVERY_PATH,
  companionDiscoveryMaxAgeSeconds: DEFAULT_COMPANION_DISCOVERY_MAX_AGE_SECONDS,
  chunkLengthSeconds: 15,
  transcriptCorrectionRules: "",
  enableLlmTranscriptCorrection: false,

  audioInputDeviceId: "default",
  audioInputDeviceLabel: "Default audio input",
  autoStopOnSilence: true,
  autoStopSilenceMinutes: DEFAULT_AUTO_STOP_SILENCE_MINUTES,
  saveRawAudio: false,
  audioSaveFolder: "Meetings/audio",

  llmProvider: "openai-compatible",
  openaiApiKey: "",
  openaiBaseUrl: "https://api.openai.com/v1",
  openaiModel: "",
  anthropicApiKey: "",
  anthropicModel: "",
  summaryLanguage: "zh",
  summaryPrompt: DEFAULT_SUMMARY_PROMPT
};

export function normalizeAutoStopSilenceMinutes(value: unknown): number {
  const parsed = typeof value === "number" ? value : Number.parseInt(String(value), 10);
  if (!Number.isInteger(parsed)) {
    return DEFAULT_AUTO_STOP_SILENCE_MINUTES;
  }
  return Math.min(MAX_AUTO_STOP_SILENCE_MINUTES, Math.max(MIN_AUTO_STOP_SILENCE_MINUTES, parsed));
}
