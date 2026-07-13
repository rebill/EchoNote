import type { EchoNoteSettings } from "../settings/settings";
import { AnthropicProvider } from "./anthropic-provider";
import type { LlmProvider } from "./llm-types";
import { OpenAiCompatibleProvider } from "./openai-compatible-provider";

export function createLlmProvider(settings: EchoNoteSettings): LlmProvider {
  if (settings.llmProvider === "anthropic") {
    if (!settings.anthropicApiKey || !settings.anthropicModel) {
      throw new Error("Anthropic API key and model are required.");
    }
    return new AnthropicProvider({
      apiKey: settings.anthropicApiKey,
      model: settings.anthropicModel
    });
  }

  if (!settings.openaiApiKey || !settings.openaiModel || !settings.openaiBaseUrl) {
    throw new Error("OpenAI-compatible API key, base URL, and model are required.");
  }
  return new OpenAiCompatibleProvider({
    apiKey: settings.openaiApiKey,
    baseUrl: settings.openaiBaseUrl,
    model: settings.openaiModel
  });
}
