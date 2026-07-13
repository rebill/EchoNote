import type { LlmProvider, LlmProviderConfig, LlmTextRequest, MeetingSummary, SummaryRequest } from "./llm-types";
import { buildSummarySystemPrompt, buildSummaryUserPrompt, parseMeetingSummary } from "./summary-json";

type AnthropicMessagesResponse = {
  content?: Array<{
    type?: string;
    text?: string;
  }>;
};

export class AnthropicProvider implements LlmProvider {
  readonly id = "anthropic" as const;

  constructor(private readonly config: LlmProviderConfig) {}

  async generateSummary(request: SummaryRequest): Promise<MeetingSummary> {
    const content = await this.generateText({
      systemPrompt: buildSummarySystemPrompt(request.language),
      userPrompt: buildSummaryUserPrompt(request.transcript, request.prompt),
      temperature: 0.2
    });

    return parseMeetingSummary(content);
  }

  async generateText(request: LlmTextRequest): Promise<string> {
    const response = await fetch("https://api.anthropic.com/v1/messages", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-api-key": this.config.apiKey,
        "anthropic-version": "2023-06-01"
      },
      body: JSON.stringify({
        model: this.config.model,
        max_tokens: 4096,
        temperature: request.temperature ?? 0.2,
        system: request.systemPrompt,
        messages: [
          {
            role: "user",
            content: request.userPrompt
          }
        ]
      })
    });

    const body = await response.text();
    if (!response.ok) {
      throw new Error(body || `Anthropic request failed with ${response.status}`);
    }

    const parsed = JSON.parse(body) as AnthropicMessagesResponse;
    const content = parsed.content?.find((item) => item.type === "text" && item.text)?.text;
    if (!content) {
      throw new Error("Anthropic response did not contain text content.");
    }

    return content;
  }
}
