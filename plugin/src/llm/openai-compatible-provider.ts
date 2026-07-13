import type { RequestUrlParam, RequestUrlResponse } from "obsidian";
import type { LlmProvider, LlmProviderConfig, LlmTextRequest, MeetingSummary, SummaryRequest } from "./llm-types";
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
    const content = await this.generateText({
      systemPrompt: buildSummarySystemPrompt(request.language),
      userPrompt: buildSummaryUserPrompt(request.transcript, request.prompt),
      temperature: 0.2
    });

    return parseMeetingSummary(content);
  }

  async generateText(request: LlmTextRequest): Promise<string> {
    const response = await requestObsidianUrl({
      url: `${stripTrailingSlash(this.config.baseUrl ?? "")}/chat/completions`,
      method: "POST",
      contentType: "application/json",
      headers: {
        Authorization: `Bearer ${this.config.apiKey}`
      },
      body: JSON.stringify({
        model: this.config.model,
        messages: [
          { role: "system", content: request.systemPrompt },
          { role: "user", content: request.userPrompt }
        ],
        temperature: request.temperature ?? 0.2
      }),
      throw: false
    });

    const body = response.text;
    if (response.status < 200 || response.status >= 300) {
      throw new Error(body || `OpenAI-compatible request failed with ${response.status}`);
    }

    const parsed = JSON.parse(body) as OpenAiChatResponse;
    const content = parsed.choices?.[0]?.message?.content;
    if (!content) {
      throw new Error("OpenAI-compatible response did not contain message content.");
    }

    return content;
  }
}

function stripTrailingSlash(value: string): string {
  return value.replace(/\/+$/, "");
}

async function requestObsidianUrl(request: RequestUrlParam): Promise<RequestUrlResponse> {
  const { requestUrl } = require("obsidian") as typeof import("obsidian");
  return requestUrl(request);
}
