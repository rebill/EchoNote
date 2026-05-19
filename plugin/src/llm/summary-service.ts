import type { EchoNoteSettings } from "../settings/settings";
import { AnthropicProvider } from "./anthropic-provider";
import type { LlmProvider, MeetingSummary } from "./llm-types";
import { OpenAiCompatibleProvider } from "./openai-compatible-provider";

const MAX_TRANSCRIPT_CHARS = 20000;

export class SummaryService {
  async summarize(transcript: string, settings: EchoNoteSettings): Promise<MeetingSummary> {
    const normalizedTranscript = transcript.trim();
    if (!normalizedTranscript) {
      throw new Error("Transcript is empty.");
    }

    const provider = this.createProvider(settings);
    if (normalizedTranscript.length <= MAX_TRANSCRIPT_CHARS) {
      return provider.generateSummary({
        transcript: normalizedTranscript,
        language: settings.summaryLanguage,
        prompt: settings.summaryPrompt
      });
    }

    const partialSummaries: MeetingSummary[] = [];
    for (const chunk of splitText(normalizedTranscript, MAX_TRANSCRIPT_CHARS)) {
      partialSummaries.push(
        await provider.generateSummary({
          transcript: chunk,
          language: settings.summaryLanguage,
          prompt: settings.summaryPrompt
        })
      );
    }

    return provider.generateSummary({
      transcript: renderPartialSummaries(partialSummaries),
      language: settings.summaryLanguage,
      prompt: "Merge these partial meeting summaries into one final structured meeting summary."
    });
  }

  private createProvider(settings: EchoNoteSettings): LlmProvider {
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
}

function splitText(text: string, maxChars: number): string[] {
  const chunks: string[] = [];
  for (let index = 0; index < text.length; index += maxChars) {
    chunks.push(text.slice(index, index + maxChars));
  }
  return chunks;
}

function renderPartialSummaries(summaries: MeetingSummary[]): string {
  return summaries
    .map((summary, index) =>
      [
        `Partial Summary ${index + 1}`,
        `Summary: ${summary.summary}`,
        `Decisions: ${summary.decisions.join("; ")}`,
        `Action Items: ${summary.actionItems.join("; ")}`,
        `Key Points: ${summary.keyPoints.join("; ")}`,
        `Open Questions: ${summary.openQuestions.join("; ")}`
      ].join("\n")
    )
    .join("\n\n");
}
