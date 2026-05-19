import type { SummaryLanguage } from "../settings/settings";

export type MeetingSummary = {
  summary: string;
  decisions: string[];
  actionItems: string[];
  keyPoints: string[];
  openQuestions: string[];
};

export type SummaryRequest = {
  transcript: string;
  language: SummaryLanguage;
  prompt: string;
};

export type LlmProviderConfig = {
  apiKey: string;
  baseUrl?: string;
  model: string;
};

export interface LlmProvider {
  id: "openai-compatible" | "anthropic";
  generateSummary(request: SummaryRequest): Promise<MeetingSummary>;
}
