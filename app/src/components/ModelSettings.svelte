<script lang="ts">
  import { bytes } from "../lib/format";
  import { errorText, t } from "../lib/i18n.svelte";
  import { models } from "../lib/models.svelte";
  import type { ModelInfo } from "../lib/types";

  let confirmDelete = $state(false);

  const accurate = $derived(models.accurate);
  // The live model and the VAD ship with the app; a download only repairs them.
  const broken = $derived(models.list.filter((model) => model.role !== "accurate" && !model.installed));

  function percent(model: ModelInfo): number {
    const progress = models.progress[model.id];
    const done = progress?.done ?? model.partial;
    return model.size > 0 ? Math.min(100, Math.floor((done * 100) / model.size)) : 0;
  }
</script>

{#snippet status(model: ModelInfo)}
  {#if model.downloading}
    <div class="row">
      <progress max="100" value={percent(model)}></progress>
      <span class="muted">
        {models.progress[model.id]?.stage === "extracting"
          ? t("models.extracting", { percent: percent(model) })
          : t("models.downloading", { percent: percent(model) })}
      </span>
      <button class="link pause-download" onclick={() => models.cancel(model.id)}>{t("models.pause")}</button>
    </div>
  {:else if !model.installed}
    <div class="row">
      <button class="download-model" onclick={() => models.download(model.id)}>
        {model.partial > 0 ? t("models.resume", { percent: percent(model) }) : t("models.download")}
      </button>
      {#if model.partial > 0 && model.role === "accurate"}
        <button class="link danger" onclick={() => models.remove(model.id)}>{t("models.discard")}</button>
      {/if}
    </div>
  {/if}
  {#if models.errors[model.id]}
    <small class="bad">{errorText(models.errors[model.id]!)}</small>
  {/if}
{/snippet}

{#if accurate}
  <div class="model accurate-model">
    <div class="head">
      <strong>{t("models.accurate")}</strong>
      <span class="muted">{t("models.size", { size: bytes(accurate.size), memory: accurate.memoryMb })}</span>
      {#if accurate.installed}
        <span class="ok installed">{t("settings.installed")}</span>
      {/if}
    </div>
    <small>{t("models.accurateHint")}</small>
    {@render status(accurate)}
    {#if accurate.installed && accurate.removable}
      {#if confirmDelete}
        <div class="row">
          <span>{t("models.deleteConfirm")}</span>
          <button class="danger delete-model" onclick={() => models.remove(accurate.id).then(() => (confirmDelete = false))}>
            {t("models.delete")}
          </button>
          <button class="link" onclick={() => (confirmDelete = false)}>{t("detail.cancel")}</button>
        </div>
      {:else}
        <button class="link danger" onclick={() => (confirmDelete = true)}>{t("models.delete")}</button>
      {/if}
    {/if}
    {#if !accurate.installed}
      <small>{t("models.network", { license: accurate.license })}</small>
    {/if}
  </div>
{/if}

{#each broken as model (model.id)}
  <div class="model">
    <div class="head">
      <strong>{model.role === "live" ? t("models.live") : t("models.vad")}</strong>
      <span class="bad">{t("settings.missing")}</span>
    </div>
    <small>{t("models.repairHint", { size: bytes(model.size) })}</small>
    {@render status(model)}
  </div>
{/each}

<style>
  .model {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 8px 0 12px;
  }
  .head {
    display: flex;
    gap: 12px;
    align-items: baseline;
  }
  .row {
    display: flex;
    gap: 12px;
    align-items: center;
  }
  progress {
    width: 200px;
  }
  small,
  .muted {
    color: var(--muted);
  }
  .ok {
    color: var(--ok);
  }
  .bad,
  .danger {
    color: var(--danger);
  }
  .link.danger {
    align-self: flex-start;
  }
</style>
