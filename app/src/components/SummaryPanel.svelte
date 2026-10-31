<script lang="ts">
  import { ai, aiErrorText, templateLabel } from "../lib/ai.svelte";
  import { dateTime } from "../lib/format";
  import { locale, t } from "../lib/i18n.svelte";
  import { api } from "../lib/ipc";
  import { withoutTitle } from "../lib/markdown";
  import type { MeetingDetail, SummaryView } from "../lib/types";
  import ConsentBanner from "./ConsentBanner.svelte";
  import Markdown from "./Markdown.svelte";
  import ProviderForm from "./ProviderForm.svelte";

  let { detail, onSeek }: { detail: MeetingDetail; onSeek?: (ms: number) => void } = $props();

  const id = $derived(detail.meeting.id);
  const settings = $derived(ai.settings);
  const provider = $derived(ai.active);
  const run = $derived(ai.run(id, "notes"));
  const running = $derived(run?.status === "running");
  // The run's result until the meeting is fetched again.
  const notes = $derived<SummaryView | null>(
    run?.status === "done" && (run.result as SummaryView).id !== detail.notes?.id
      ? (run.result as SummaryView)
      : detail.notes,
  );
  const templates = $derived(
    settings
      ? [...settings.builtinTemplates, ...settings.templates.map((custom) => `custom:${custom.id}`)]
      : [],
  );
  const template = $derived(settings && templates.includes(settings.template) ? settings.template : "general");

  function generate() {
    ai.start(id, "notes", () => api.summarize(id, template));
  }

  const stage = $derived.by(() => {
    const stage = run?.stage;
    if (!running) return null;
    if (!stage) return t("summary.starting");
    switch (stage.stage) {
      case "parts":
        return t("summary.parts", { done: stage.done, total: stage.total });
      case "merging":
        return t("summary.merging", { notes: stage.notes });
      default:
        return t("summary.writing");
    }
  });
</script>

<div class="panel summary-panel">
  {#if settings && !provider}
    <div class="setup">
      <h3>{t("ai.setupTitle")}</h3>
      <p class="muted">{t("ai.setupBody")}</p>
      <ProviderForm compact onSaved={generate} />
      <p class="muted">{t("ai.setupInSettings")}</p>
    </div>
  {:else if provider}
    <div class="toolbar">
      <label>
        <span class="muted">{t("summary.template")}</span>
        <select
          class="template"
          value={template}
          disabled={running}
          onchange={(event) => ai.setOptions({ template: (event.currentTarget as HTMLSelectElement).value })}
        >
          {#each templates as name (name)}
            <option value={name}>{templateLabel(name, settings)}</option>
          {/each}
        </select>
      </label>
      {#if running}
        <button class="stop-notes" disabled={run?.job == null} onclick={() => ai.cancel(id, "notes")}>
          {t("summary.stop")}
        </button>
        <span class="muted">{stage}</span>
      {:else}
        <button class="primary generate" onclick={generate}>
          {notes ? t("summary.regenerate") : t("summary.generate")}
        </button>
      {/if}
      <span class="disclosure">
        {provider.local
          ? t("ai.disclosureLocal", { provider: provider.name, model: provider.model })
          : t("ai.disclosure", { provider: provider.name, model: provider.model })}
      </span>
    </div>
    <ConsentBanner meeting={id} kinds={["notes"]} />
    {#if run?.status === "error" && run.error}
      <div class="error">{aiErrorText(run.error)}</div>
    {/if}
  {/if}

  {#if running && run}
    <div class="notes streaming">
      <Markdown text={withoutTitle(run.text)} />
    </div>
  {:else if notes}
    <div class="notes">
      <Markdown text={notes.content} {onSeek} />
      <p class="by">
        {t("summary.by", { provider: notes.provider, time: dateTime(notes.createdAtMs, locale.value) })}
        · {templateLabel(notes.template, settings)}
      </p>
      {#if notes.truncated}<p class="warn">{t("summary.truncated")}</p>{/if}
    </div>
  {:else if provider}
    <p class="muted empty">{t("summary.empty")}</p>
  {/if}

  {#if detail.highlights?.content}
    <details class="highlights">
      <summary>{t("summary.highlights")}</summary>
      <Markdown text={detail.highlights.content} {onSeek} />
    </details>
  {/if}
</div>

<style>
  .panel {
    height: 100%;
    overflow-y: auto;
    padding: 12px 20px 32px;
  }
  .setup {
    max-width: 560px;
  }
  .setup h3 {
    margin: 4px 0;
    font-size: 15px;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: center;
  }
  .toolbar label {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .disclosure {
    margin-left: auto;
    font-size: 12px;
    color: var(--muted);
  }
  .primary {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }
  .muted {
    color: var(--muted);
    font-size: 13px;
  }
  .empty {
    margin-top: 16px;
  }
  .error {
    margin: 8px 0;
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
    background: var(--error-bg);
  }
  .notes {
    margin-top: 8px;
    max-width: 760px;
  }
  .by {
    margin-top: 16px;
    font-size: 12px;
    color: var(--muted);
  }
  .warn {
    font-size: 13px;
    color: var(--danger);
  }
  .highlights {
    margin-top: 20px;
    max-width: 760px;
  }
  .highlights summary {
    cursor: pointer;
    color: var(--muted);
    font-size: 13px;
  }
</style>
