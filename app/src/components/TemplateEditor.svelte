<script lang="ts">
  import { ai } from "../lib/ai.svelte";
  import { app } from "../lib/app.svelte";
  import { t } from "../lib/i18n.svelte";
  import { api, asCommandError } from "../lib/ipc";
  import type { Template } from "../lib/types";

  let editing = $state<number | null>(null);
  let name = $state("");
  let instructions = $state("");
  const templates = $derived(ai.settings?.templates ?? []);

  function edit(template: Template) {
    editing = template.id;
    name = template.name;
    instructions = template.instructions;
  }

  function reset() {
    editing = null;
    name = "";
    instructions = "";
  }

  async function save() {
    if (!name.trim() || !instructions.trim()) return;
    try {
      ai.settings = await api.saveTemplate(editing, name, instructions);
      reset();
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }

  async function remove(id: number) {
    try {
      ai.settings = await api.deleteTemplate(id);
      if (editing === id) reset();
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }
</script>

<p class="hint">{t("settings.templatesHint")}</p>
<form
  class="template-form"
  onsubmit={(event) => {
    event.preventDefault();
    void save();
  }}
>
  <input class="template-name" placeholder={t("template.name")} bind:value={name} maxlength="60" />
  <textarea
    class="template-instructions"
    placeholder={t("template.instructions")}
    bind:value={instructions}
    maxlength="4000"
    rows="5"
  ></textarea>
  <span class="buttons">
    <button type="submit" disabled={!name.trim() || !instructions.trim()}>
      {editing == null ? t("template.add") : t("template.save")}
    </button>
    {#if editing != null}
      <button type="button" class="link" onclick={reset}>{t("template.cancel")}</button>
    {/if}
  </span>
</form>
{#if templates.length === 0}
  <p class="hint">{t("settings.templatesEmpty")}</p>
{:else}
  <ul class="templates">
    {#each templates as template (template.id)}
      <li class:active={editing === template.id}>
        <span class="name">{template.name}</span>
        <span class="preview">{template.instructions}</span>
        <button class="link" onclick={() => edit(template)}>{t("provider.edit")}</button>
        <button class="link danger" onclick={() => remove(template.id)}>{t("template.delete")}</button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .hint {
    margin: 4px 0 8px;
    font-size: 13px;
    color: var(--muted);
  }
  .template-form {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  textarea {
    font: inherit;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: inherit;
    resize: vertical;
  }
  .buttons {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .templates {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    border-top: 1px solid var(--border);
  }
  .templates li {
    display: grid;
    grid-template-columns: 140px 1fr auto auto;
    gap: 12px;
    align-items: baseline;
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }
  .templates li.active {
    background: var(--highlight);
  }
  .name {
    font-weight: 600;
    word-break: break-word;
  }
  .preview {
    color: var(--muted);
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .danger {
    color: var(--danger);
  }
</style>
