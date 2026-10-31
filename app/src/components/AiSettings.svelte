<script lang="ts">
  import { ai } from "../lib/ai.svelte";
  import { app } from "../lib/app.svelte";
  import { t } from "../lib/i18n.svelte";
  import { api, asCommandError } from "../lib/ipc";
  import type { Provider } from "../lib/types";
  import ProviderForm from "./ProviderForm.svelte";
  import TemplateEditor from "./TemplateEditor.svelte";

  /// The provider being edited, "new", or none.
  let editing = $state<string | null>(null);
  let confirming = $state<string | null>(null);
  const settings = $derived(ai.settings);

  async function remove(provider: Provider) {
    confirming = null;
    try {
      ai.settings = await api.deleteProvider(provider.id);
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }
</script>

{#if settings}
  <p class="hint">{t("settings.aiHint")}</p>
  {#if settings.providers.length === 0 && editing == null}
    <p class="hint">{t("settings.aiEmpty")}</p>
  {/if}
  <ul class="providers">
    {#each settings.providers as provider (provider.id)}
      <li>
        {#if editing === provider.id}
          <ProviderForm {provider} onSaved={() => (editing = null)} onCancel={() => (editing = null)} />
        {:else}
          <div class="provider">
            <label class="use">
              <input
                type="radio"
                name="active-provider"
                checked={settings.active === provider.id}
                onchange={() => ai.setOptions({ active: provider.id })}
              />
              <span class="name">{provider.name}</span>
            </label>
            <span class="model">{provider.model}</span>
            <span class="where">{provider.local ? t("settings.aiLocal") : (provider.origin ?? "")}</span>
            <span class="actions">
              {#if settings.active === provider.id}<span class="active">{t("settings.aiActive")}</span>{/if}
              <button class="link" onclick={() => (editing = provider.id)}>{t("provider.edit")}</button>
              <button class="link danger" onclick={() => (confirming = provider.id)}>{t("provider.delete")}</button>
            </span>
          </div>
          {#if confirming === provider.id}
            <div class="confirm" role="alertdialog">
              <span>{t("settings.aiDeleteConfirm", { name: provider.name })}</span>
              <button class="danger" onclick={() => remove(provider)}>{t("provider.delete")}</button>
              <button class="link" onclick={() => (confirming = null)}>{t("provider.cancel")}</button>
            </div>
          {/if}
        {/if}
      </li>
    {/each}
  </ul>
  {#if editing === "new"}
    <div class="new">
      <ProviderForm onSaved={() => (editing = null)} onCancel={() => (editing = null)} />
    </div>
  {:else}
    <button class="add-provider" onclick={() => (editing = "new")}>{t("settings.aiAdd")}</button>
  {/if}

  <label class="check">
    <input
      type="checkbox"
      class="live-highlights"
      checked={settings.liveHighlights}
      onchange={(event) => ai.setLiveHighlights((event.currentTarget as HTMLInputElement).checked, null)}
    />
    <span>{t("highlights.toggle")}</span>
  </label>
  <small class="hint">{t("highlights.hint")}</small>
  {#if ai.consent?.kind === "highlights" && ai.consent.meeting === 0}
    <div class="confirm" role="alertdialog">
      <span>{t("ai.consentBody", { origin: ai.consent.provider.origin ?? ai.consent.provider.baseUrl })}</span>
      <button onclick={() => ai.agree()}>{t("ai.consentYes")}</button>
      <button class="link" onclick={() => (ai.consent = null)}>{t("ai.consentNo")}</button>
    </div>
  {/if}

  <h3>{t("settings.templates")}</h3>
  <TemplateEditor />
{/if}

<style>
  .hint {
    display: block;
    margin: 4px 0 8px;
    font-size: 13px;
    color: var(--muted);
  }
  h3 {
    font-size: 13px;
    margin: 20px 0 6px;
  }
  .providers {
    list-style: none;
    margin: 0 0 8px;
    padding: 0;
  }
  .providers li {
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }
  .provider {
    display: grid;
    grid-template-columns: minmax(120px, 1fr) minmax(80px, 1fr) minmax(80px, 1fr) auto;
    gap: 12px;
    align-items: center;
  }
  .use {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .name {
    font-weight: 600;
  }
  .model,
  .where {
    color: var(--muted);
    font-size: 13px;
    word-break: break-all;
  }
  .actions {
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 13px;
  }
  .active {
    color: var(--ok);
  }
  .new {
    padding: 8px 0;
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 16px 0 0;
  }
  .confirm {
    display: flex;
    gap: 12px;
    align-items: center;
    margin: 6px 0;
    padding: 8px 12px;
    border-radius: 6px;
    background: var(--warn-bg);
    font-size: 13px;
  }
  .confirm span {
    flex: 1;
  }
  .danger {
    color: var(--danger);
  }
</style>
