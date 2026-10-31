// AI settings and runs. A run lives here, not in a view, so streamed notes survive switching
// meetings or tabs; finished results come back with the meeting (`get_meeting`).

import { app } from "./app.svelte";
import { errorText, t } from "./i18n.svelte";
import { api, asCommandError, events } from "./ipc";
import { zh, type StringKey } from "./strings";
import type {
  AiEvent,
  AiKind,
  AiSettings,
  CommandError,
  CorrectOutcome,
  Provider,
  RetranscribeOutcome,
  Stage,
  SummaryView,
} from "./types";

export type RunKind = Exclude<AiKind, "highlights">;

export interface Run {
  /// Null until the command returns (events can arrive first).
  job: number | null;
  status: "running" | "done" | "error";
  text: string;
  stage: Stage | null;
  /// The question, for answers.
  question: string | null;
  result: unknown;
  error: CommandError | null;
}

export interface Highlights {
  content: string;
  updatedAtMs: number | null;
  error: CommandError | null;
}

/// A run waiting for the user to agree to send the transcript.
export interface Consent {
  provider: Provider;
  meeting: number;
  kind: RunKind | "highlights";
  start: () => void;
}

/// Errors where the server's own words help (a 400's reason, a bad model name).
const DETAILED = new Set(["llm-rejected", "llm-server", "llm-bad-response", "llm-not-found", "llm-unauthorized", "keychain"]);

export function aiErrorText(error: CommandError): string {
  const text = errorText(error);
  return DETAILED.has(error.code) && error.message ? `${text}（${error.message}）` : text;
}

function key(meeting: number, kind: RunKind): string {
  return `${meeting}:${kind}`;
}

class AiStore {
  settings = $state.raw<AiSettings | null>(null);
  runs = $state.raw<Record<string, Run>>({});
  highlights = $state.raw<Record<number, Highlights>>({});
  consent = $state.raw<Consent | null>(null);

  async init(): Promise<void> {
    await events.ai((event) => this.onEvent(event));
    await this.load();
    const running = await api.runningJobs().catch(() => []);
    for (const job of running) {
      if (job.kind === "highlights") continue;
      this.set(job.meeting, job.kind, {
        job: job.job,
        status: "running",
        text: "",
        stage: null,
        question: null,
        result: null,
        error: null,
      });
    }
  }

  async load(): Promise<void> {
    try {
      this.settings = await api.getAi();
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }

  get active(): Provider | null {
    const settings = this.settings;
    return settings?.providers.find((provider) => provider.id === settings.active) ?? null;
  }

  run(meeting: number, kind: RunKind): Run | null {
    return this.runs[key(meeting, kind)] ?? null;
  }

  running(meeting: number, kind: RunKind): boolean {
    return this.run(meeting, kind)?.status === "running";
  }

  private set(meeting: number, kind: RunKind, run: Run | null): void {
    const runs = { ...this.runs };
    if (run) runs[key(meeting, kind)] = run;
    else delete runs[key(meeting, kind)];
    this.runs = runs;
  }

  private patch(meeting: number, kind: RunKind, change: Partial<Run>): void {
    const run = this.run(meeting, kind);
    if (run) this.set(meeting, kind, { ...run, ...change });
  }

  /// Start a run, asking first when the transcript would go to a new server.
  start(meeting: number, kind: RunKind, invoke: () => Promise<number>, question: string | null = null): void {
    const provider = this.active;
    if (!provider) {
      app.showError({ code: "no-provider", message: "" });
      return;
    }
    if (provider.needsConsent) {
      this.consent = { provider, meeting, kind, start: () => this.start(meeting, kind, invoke, question) };
      return;
    }
    this.begin(meeting, kind, invoke, question);
  }

  /// Start a run that sends nothing anywhere (the high-accuracy model).
  startLocal(meeting: number, kind: RunKind, invoke: () => Promise<number>): void {
    this.begin(meeting, kind, invoke, null);
  }

  private begin(meeting: number, kind: RunKind, invoke: () => Promise<number>, question: string | null): void {
    this.set(meeting, kind, { job: null, status: "running", text: "", stage: null, question, result: null, error: null });
    invoke()
      .then((job) => {
        const run = this.run(meeting, kind);
        if (run && run.job == null) this.patch(meeting, kind, { job });
      })
      .catch((error) => {
        const failure = asCommandError(error);
        if (failure.code === "consent-required") void this.load();
        this.patch(meeting, kind, { status: "error", error: failure });
      });
  }

  cancel(meeting: number, kind: RunKind): void {
    const job = this.run(meeting, kind)?.job;
    if (job != null) void api.cancelJob(job);
  }

  /// Forget a finished run (its result is in the meeting now).
  clear(meeting: number, kind: RunKind): void {
    if (!this.running(meeting, kind)) this.set(meeting, kind, null);
  }

  async agree(): Promise<void> {
    const consent = this.consent;
    if (!consent) return;
    this.consent = null;
    try {
      this.settings = await api.consentProvider(consent.provider.id);
      consent.start();
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }

  async setOptions(patch: Partial<Pick<AiSettings, "active" | "template" | "liveHighlights">>): Promise<boolean> {
    const settings = this.settings;
    if (!settings) return false;
    const next = { active: settings.active, template: settings.template, liveHighlights: settings.liveHighlights, ...patch };
    try {
      this.settings = await api.setAiOptions(next.active, next.template, next.liveHighlights);
      return true;
    } catch (error) {
      app.showError(asCommandError(error));
      return false;
    }
  }

  /// Turn live highlights on (asking for consent when needed) or off.
  async setLiveHighlights(on: boolean, meeting: number | null): Promise<void> {
    const provider = this.active;
    if (on && !provider) {
      app.toast = t("ai.needProvider");
      return;
    }
    if (on && provider?.needsConsent) {
      this.consent = {
        provider,
        meeting: meeting ?? 0,
        kind: "highlights",
        start: () => void this.setOptions({ liveHighlights: true }),
      };
      return;
    }
    await this.setOptions({ liveHighlights: on });
  }

  async loadHighlights(meeting: number): Promise<void> {
    if (this.highlights[meeting]) return;
    const saved = await api.getHighlights(meeting).catch(() => null);
    if (saved && !this.highlights[meeting]) this.setHighlights(meeting, fromSummary(saved));
  }

  private setHighlights(meeting: number, highlights: Highlights): void {
    this.highlights = { ...this.highlights, [meeting]: highlights };
  }

  private onEvent(event: AiEvent): void {
    if (event.kind === "highlights") {
      const current = this.highlights[event.meeting] ?? { content: "", updatedAtMs: null, error: null };
      if (event.event === "done") this.setHighlights(event.meeting, fromSummary(event.result as SummaryView));
      else if (event.event === "error") this.setHighlights(event.meeting, { ...current, error: event.error });
      return;
    }
    const kind = event.kind;
    const run = this.run(event.meeting, kind);
    // Leftovers of a run this one replaced.
    if (run && run.job != null && run.job !== event.job) return;
    if (!run) {
      // A run this window did not start (opened while it ran): follow it from here.
      if (event.event === "done") app.libraryVersion++;
      if (event.event === "done" || event.event === "error") return;
      this.set(event.meeting, kind, { job: event.job, status: "running", text: "", stage: null, question: null, result: null, error: null });
    }
    switch (event.event) {
      case "text":
        this.patch(event.meeting, kind, { job: event.job, text: (this.run(event.meeting, kind)?.text ?? "") + event.text });
        break;
      case "progress":
        this.patch(event.meeting, kind, { job: event.job, stage: event });
        break;
      case "done":
        this.patch(event.meeting, kind, { job: event.job, status: "done", result: event.result });
        // The meeting view reloads and shows what was saved.
        app.libraryVersion++;
        if (kind === "correct") this.announce(event.result as CorrectOutcome);
        if (kind === "retranscribe") this.announceRetranscribed(event.result as RetranscribeOutcome);
        break;
      case "error":
        // A stopped pass keeps what it finished: show it.
        if (kind === "retranscribe") app.libraryVersion++;
        if (event.error.code === "cancelled") this.set(event.meeting, kind, null);
        else this.patch(event.meeting, kind, { job: event.job, status: "error", error: event.error });
        break;
    }
  }

  private announceRetranscribed(outcome: RetranscribeOutcome): void {
    app.toast = outcome.total === 0 ? t("retranscribe.nothing") : t("retranscribe.done", { changed: outcome.changed, total: outcome.total });
  }

  private announce(outcome: CorrectOutcome): void {
    const parts = [outcome.applied > 0 ? t("correct.done", { applied: outcome.applied }) : t("correct.none")];
    if (outcome.rejected > 0) parts.push(t("correct.rejected", { n: outcome.rejected }));
    if (outcome.failedBatches > 0) parts.push(t("correct.failed", { n: outcome.failedBatches }));
    app.toast = parts.join(" ");
  }
}

function fromSummary(summary: SummaryView): Highlights {
  return { content: summary.content, updatedAtMs: summary.createdAtMs, error: null };
}

export const ai = new AiStore();

export function templateLabel(template: string, settings: AiSettings | null): string {
  if (template.startsWith("custom:")) {
    const id = Number(template.slice(7));
    return settings?.templates.find((custom) => custom.id === id)?.name ?? template;
  }
  const name = `template.${template}`;
  return name in zh ? t(name as StringKey) : template;
}
