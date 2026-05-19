import type { LlmProvider, LlmProviderConfig, MeetingSummary, SummaryRequest } from "./llm-types";
import { buildSummarySystemPrompt, buildSummaryUserPrompt, parseMeetingSummary } from "./summary-json";

type OpenAiChatResponse = {
  choices?: Array<{
    message?: {
      content?: string;
    };
  }>;
};

export class OpenAiCompatibleProvider implements LlmProvider {
  readonly id = "openai-compatible" as const;

  constructor(private readonly config: LlmProviderConfig) {}

  async generateSummary(request: SummaryRequest): Promise<MeetingSummary> {
    const response = await fetch(`${stripTrailingSlash(this.config.baseUrl ?? "")}/chat/completions`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.config.apiKey}`
      },
      body: JSON.stringify({
        model: this.config.model,
        messages: [
          { role: "system", content: buildSummarySystemPrompt(request.language) },
          { role: "user", content: buildSummaryUserPrompt(request.transcript, request.prompt) }
        ],
        temperature: 0.2
      })
    });

    const body = await response.text();
    if (!response.ok) {
      throw new Error(body || `OpenAI-compatible request failed with ${response.status}`);
    }

    const parsed = JSON.parse(body) as OpenAiChatResponse;
    const content = parsed.choices?.[0]?.message?.content;
    if (!content) {
      throw new Error("OpenAI-compatible response did not contain message content.");
    }

    return parseMeetingSummary(content);
  }
}

function stripTrailingSlash(value: string): string {
  return value.replace(/\/+$/, "");
}
