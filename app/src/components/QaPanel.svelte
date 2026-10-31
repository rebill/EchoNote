<script lang="ts">
  import { ai, aiErrorText } from "../lib/ai.svelte";
  import { app } from "../lib/app.svelte";
  import { t } from "../lib/i18n.svelte";
  import { api, asCommandError } from "../lib/ipc";
  import type { MeetingDetail, QaTurn } from "../lib/types";
  import ConsentBanner from "./ConsentBanner.svelte";
  import Markdown from "./Markdown.svelte";

  let { detail, onSeek }: { detail: MeetingDetail; onSeek?: (ms: number) => void } = $props();

  let question = $state("");
  const id = $derived(detail.meeting.id);
  const provider = $derived(ai.active);
  const run = $derived(ai.run(id, "answer"));
  const running = $derived(run?.status === "running");
  // The run's answer until the meeting is fetched again.
  const turns = $derived.by<QaTurn[]>(() => {
    const done = run?.status === "done" ? (run.result as QaTurn) : null;
    return done && !detail.qa.some((turn) => turn.id === done.id) ? [...detail.qa, done] : detail.qa;
  });
  const last = $derived(run?.status === "done" ? (run.result as QaTurn) : null);
  let list = $state<HTMLDivElement>();

  $effect(() => {
    void turns.length;
    void run?.text;
    if (list) list.scrollTop = list.scrollHeight;
  });

  function ask() {
    const text = question.trim();
    if (!text || running) return;
    ai.start(id, "answer", () => api.ask(id, text), text);
    if (!ai.consent) question = "";
  }

  async function clear() {
    try {
      await api.clearQa(id);
      ai.clear(id, "answer");
      app.libraryVersion++;
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }
</script>

<div class="panel qa-panel">
  <div class="turns" bind:this={list}>
    {#if turns.length === 0 && !running}
      <p class="muted">{provider ? t("qa.empty") : t("ai.needProvider")}</p>
    {/if}
    {#each turns as turn (turn.id)}
      <div class="turn">
        <p class="question">{turn.question}</p>
        <div class="answer"><Markdown text={turn.answer} {onSeek} /></div>
        {#if last?.id === turn.id && last.excerpt}<p class="note">{t("qa.excerpt")}</p>{/if}
        {#if last?.id === turn.id && last.truncated}<p class="note">{t("qa.truncated")}</p>{/if}
      </div>
    {/each}
    {#if run && run.status !== "done" && run.question}
      <div class="turn pending">
        <p class="question">{run.question}</p>
        {#if run.status === "error" && run.error}
          <div class="error">{aiErrorText(run.error)}</div>
        {:else}
          <div class="answer"><Markdown text={run.text || "…"} /></div>
        {/if}
      </div>
    {/if}
  </div>

  <ConsentBanner meeting={id} kinds={["answer"]} />
  <form
    class="ask"
    onsubmit={(event) => {
      event.preventDefault();
      ask();
    }}
  >
    <input
      class="question-input"
      bind:value={question}
      maxlength="1000"
      placeholder={t("qa.placeholder")}
      disabled={!provider}
    />
    {#if running}
      <button type="button" disabled={run?.job == null} onclick={() => ai.cancel(id, "answer")}>{t("qa.stop")}</button>
    {:else}
      <button type="submit" class="primary ask-button" disabled={!provider || !question.trim()}>{t("qa.ask")}</button>
    {/if}
    {#if turns.length > 0 && !running}
      <button type="button" class="link clear-qa" onclick={clear}>{t("qa.clear")}</button>
    {/if}
  </form>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .turns {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 12px 20px;
  }
  .turn {
    max-width: 760px;
    margin-bottom: 16px;
  }
  .question {
    display: inline-block;
    margin: 0 0 4px;
    padding: 6px 10px;
    border-radius: 8px;
    background: var(--selected);
    white-space: pre-wrap;
    user-select: text;
  }
  .answer {
    padding-left: 2px;
  }
  .note {
    margin: 2px 0;
    font-size: 12px;
    color: var(--muted);
  }
  .error {
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
    background: var(--error-bg);
  }
  .ask {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 10px 20px 14px;
    border-top: 1px solid var(--border);
  }
  .ask input {
    flex: 1;
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
  .qa-panel :global(.consent) {
    margin: 0 20px;
  }
</style>
