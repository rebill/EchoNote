import type { EchoNoteSettings } from "../settings/settings";
import { createLlmProvider } from "./llm-provider-factory";
import type { LlmProvider, MeetingSummary, SummaryRequest } from "./llm-types";
import { groupByRenderedBudget, mapWithConcurrency, splitTextAtBoundaries } from "./summary-planner";

const MAX_TRANSCRIPT_CHARS = 20000;
const MAX_MERGE_INPUT_CHARS = 20000;
const SUMMARY_CONCURRENCY = 2;
const SUMMARY_ATTEMPTS = 2;

export type SummaryProgress = {
  stage: "partial" | "merge";
  completed: number;
  total: number;
};

type SummaryServiceOptions = {
  chunkChars: number;
  mergeInputChars: number;
  concurrency: number;
  attempts: number;
  providerFactory: typeof createLlmProvider;
};

export class SummaryService {
  private readonly options: SummaryServiceOptions;

  constructor(options: Partial<SummaryServiceOptions> = {}) {
    this.options = {
      chunkChars: options.chunkChars ?? MAX_TRANSCRIPT_CHARS,
      mergeInputChars: options.mergeInputChars ?? MAX_MERGE_INPUT_CHARS,
      concurrency: options.concurrency ?? SUMMARY_CONCURRENCY,
      attempts: options.attempts ?? SUMMARY_ATTEMPTS,
      providerFactory: options.providerFactory ?? createLlmProvider
    };
  }

  async summarize(
    transcript: string,
    settings: EchoNoteSettings,
    onProgress?: (progress: SummaryProgress) => void
  ): Promise<MeetingSummary> {
    const normalizedTranscript = transcript.trim();
    if (!normalizedTranscript) {
      throw new Error("Transcript is empty.");
    }

    const provider = this.options.providerFactory(settings);
    if (normalizedTranscript.length <= this.options.chunkChars) {
      return this.generateWithRetry(provider, {
        transcript: normalizedTranscript,
        language: settings.summaryLanguage,
        prompt: settings.summaryPrompt
      });
    }

    const chunks = splitTextAtBoundaries(normalizedTranscript, this.options.chunkChars);
    let completed = 0;
    const partialSummaries = await mapWithConcurrency(
      chunks,
      this.options.concurrency,
      async (chunk) => {
        const summary = await this.generateWithRetry(provider, {
          transcript: chunk,
          language: settings.summaryLanguage,
          prompt: settings.summaryPrompt
        });
        completed += 1;
        onProgress?.({ stage: "partial", completed, total: chunks.length });
        return summary;
      }
    );

    return this.mergeSummaries(provider, partialSummaries, settings, onProgress);
  }

  private async mergeSummaries(
    provider: LlmProvider,
    summaries: MeetingSummary[],
    settings: EchoNoteSettings,
    onProgress?: (progress: SummaryProgress) => void
  ): Promise<MeetingSummary> {
    let current = summaries;
    while (current.length > 1) {
      const groups = groupByRenderedBudget(
        current,
        this.options.mergeInputChars,
        (summary, index) => renderPartialSummary(summary, index)
      );
      if (groups.length >= current.length && current.length > 1) {
        throw new Error("Partial summaries cannot be reduced within the configured merge budget.");
      }

      let completed = 0;
      current = await mapWithConcurrency(groups, this.options.concurrency, async (group) => {
        const summary = await this.generateWithRetry(provider, {
          transcript: renderPartialSummaries(group),
          language: settings.summaryLanguage,
          prompt: "Merge these partial meeting summaries into one final structured meeting summary."
        });
        completed += 1;
        onProgress?.({ stage: "merge", completed, total: groups.length });
        return summary;
      });
    }
    return current[0];
  }

  private async generateWithRetry(provider: LlmProvider, request: SummaryRequest): Promise<MeetingSummary> {
    let lastError: unknown;
    for (let attempt = 1; attempt <= this.options.attempts; attempt += 1) {
      try {
        return await provider.generateSummary(request);
      } catch (error) {
        lastError = error;
        if (attempt >= this.options.attempts || isConfigurationError(error)) {
          throw error;
        }
      }
    }
    throw lastError;
  }
}

function renderPartialSummaries(summaries: MeetingSummary[]): string {
  return summaries
    .map((summary, index) => renderPartialSummary(summary, index))
    .join("\n\n");
}

function renderPartialSummary(summary: MeetingSummary, index: number): string {
  return [
    `Partial Summary ${index + 1}`,
    `Meeting Title: ${summary.meetingTitle}`,
    `Summary: ${summary.summary}`,
    `Decisions: ${summary.decisions.join("; ")}`,
    `Action Items: ${summary.actionItems.join("; ")}`,
    `Key Points: ${summary.keyPoints.join("; ")}`,
    `Open Questions: ${summary.openQuestions.join("; ")}`
  ].join("\n");
}

function isConfigurationError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return /api key|base url|model (?:is|are) required/i.test(message);
}
