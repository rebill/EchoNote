// Provider presets for the settings form. Models change often: the suggestion is a starting
// point, and "List models" asks the server what it offers.

import type { Api, Provider, ProviderInput } from "./types";

export interface Preset {
  id: string;
  api: Api;
  baseUrl: string;
  model: string;
  contextTokens: number;
  maxOutputTokens: number;
  /// Runs on this computer and usually needs no key.
  local: boolean;
}

export const PRESETS: Preset[] = [
  { id: "openai", api: "openAi", baseUrl: "https://api.openai.com/v1", model: "gpt-4o-mini", contextTokens: 128_000, maxOutputTokens: 8_192, local: false },
  { id: "anthropic", api: "anthropic", baseUrl: "https://api.anthropic.com/v1", model: "claude-sonnet-4-5", contextTokens: 200_000, maxOutputTokens: 8_192, local: false },
  { id: "deepseek", api: "openAi", baseUrl: "https://api.deepseek.com/v1", model: "deepseek-chat", contextTokens: 64_000, maxOutputTokens: 8_192, local: false },
  { id: "qwen", api: "openAi", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus", contextTokens: 128_000, maxOutputTokens: 8_192, local: false },
  { id: "kimi", api: "openAi", baseUrl: "https://api.moonshot.cn/v1", model: "kimi-latest", contextTokens: 128_000, maxOutputTokens: 8_192, local: false },
  { id: "zhipu", api: "openAi", baseUrl: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4-flash", contextTokens: 128_000, maxOutputTokens: 4_096, local: false },
  // Ollama's default context is small unless the model is configured for more.
  { id: "ollama", api: "openAi", baseUrl: "http://localhost:11434/v1", model: "qwen2.5:7b", contextTokens: 4_096, maxOutputTokens: 1_024, local: true },
  { id: "lmstudio", api: "openAi", baseUrl: "http://localhost:1234/v1", model: "", contextTokens: 8_192, maxOutputTokens: 2_048, local: true },
  { id: "custom", api: "openAi", baseUrl: "", model: "", contextTokens: 32_000, maxOutputTokens: 4_096, local: false },
];

export function preset(id: string): Preset {
  return PRESETS.find((p) => p.id === id) ?? PRESETS[PRESETS.length - 1]!;
}

/// The form for a saved provider, secrets left blank (kept unless replaced).
export function inputFor(provider: Provider): ProviderInput {
  return {
    id: provider.id,
    name: provider.name,
    preset: provider.preset,
    api: provider.api,
    baseUrl: provider.baseUrl,
    model: provider.model,
    contextTokens: provider.contextTokens,
    maxOutputTokens: provider.maxOutputTokens,
    proxy: provider.proxy,
    allowHttp: provider.allowHttp,
    apiKey: null,
    headers: provider.headerNames.map((name) => [name, ""]),
  };
}
