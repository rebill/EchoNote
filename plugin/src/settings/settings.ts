export const ASR_MODEL_QWEN3_0_6B_4BIT = "mlx-community/Qwen3-ASR-0.6B-4bit" as const;
export const ASR_MODEL_QWEN3_1_7B_4BIT = "mlx-community/Qwen3-ASR-1.7B-4bit" as const;

export type LlmProviderType = "openai-compatible" | "anthropic";

export type AsrModelPreset =
  | typeof ASR_MODEL_QWEN3_0_6B_4BIT
  | typeof ASR_MODEL_QWEN3_1_7B_4BIT
  | "custom";

export type ChunkLengthSeconds = 10 | 15 | 30;
export type SummaryLanguage = "auto" | "zh" | "en";

export type EchoNoteSettings = {
  meetingFolder: string;
  meetingTitleFormat: string;
  meetingTemplate: string;
  enableTimestamps: boolean;

  asrModelPreset: AsrModelPreset;
  customAsrModelId: string;
  pythonPath: string;
  asrServicePath: string;
  asrServicePort: number;
  chunkLengthSeconds: ChunkLengthSeconds;
  autoStartAsrService: boolean;

  audioInputDeviceId: string;
  audioInputDeviceLabel: string;
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

export const DEFAULT_SUMMARY_PROMPT = `You are EchoNote's meeting summarizer. Return structured JSON with summary, decisions, actionItems, keyPoints, and openQuestions.`;

export const DEFAULT_SETTINGS: EchoNoteSettings = {
  meetingFolder: "Meetings",
  meetingTitleFormat: "YYYY-MM-DD HH-mm Meeting",
  meetingTemplate: DEFAULT_MEETING_TEMPLATE,
  enableTimestamps: true,

  asrModelPreset: ASR_MODEL_QWEN3_0_6B_4BIT,
  customAsrModelId: "",
  pythonPath: "python3",
  asrServicePath: "../asr-service",
  asrServicePort: 8765,
  chunkLengthSeconds: 15,
  autoStartAsrService: true,

  audioInputDeviceId: "default",
  audioInputDeviceLabel: "Default audio input",
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

export function resolveAsrModelId(settings: EchoNoteSettings): string {
  return settings.asrModelPreset === "custom"
    ? settings.customAsrModelId.trim()
    : settings.asrModelPreset;
}
