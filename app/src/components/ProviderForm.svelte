<script lang="ts">
  import { untrack } from "svelte";

  import { ai, aiErrorText } from "../lib/ai.svelte";
  import { t } from "../lib/i18n.svelte";
  import { api, asCommandError } from "../lib/ipc";
  import { PRESETS, inputFor, preset } from "../lib/presets";
  import type { Provider, ProviderInput } from "../lib/types";

  let {
    provider = null,
    compact = false,
    submitLabel,
    onSaved,
    onCancel,
  }: {
    provider?: Provider | null;
    /// The first-use form in the summary tab: fewer fields, test before saving.
    compact?: boolean;
    submitLabel?: string;
    onSaved?: (id: string) => void;
    onCancel?: () => void;
  } = $props();

  let form = $state<ProviderInput>(
    untrack(() =>
      provider
        ? inputFor(provider)
        : {
            id: null,
            name: t("preset.openai"),
            preset: "openai",
            api: "openAi",
            baseUrl: preset("openai").baseUrl,
            model: preset("openai").model,
            contextTokens: preset("openai").contextTokens,
            maxOutputTokens: preset("openai").maxOutputTokens,
            proxy: null,
            allowHttp: false,
            apiKey: null,
            headers: [],
          },
    ),
  );
  const hasKey = untrack(() => provider?.hasKey ?? false);
  let key = $state("");
  let clearKey = $state(false);
  let busy = $state(false);
  let status = $state<{ ok: boolean; text: string } | null>(null);
  let models = $state<string[]>([]);
  const local = $derived(preset(form.preset).local);
  const showAddress = $derived(!compact || ["custom", "ollama", "lmstudio"].includes(form.preset));
  const ready = $derived(form.name.trim() !== "" && form.baseUrl.trim() !== "" && form.model.trim() !== "");
  const listId = `models-${Math.random().toString(36).slice(2)}`;

  function choose(id: string) {
    const p = preset(id);
    form = {
      ...form,
      preset: id,
      name: t(`preset.${id}` as "preset.custom"),
      api: p.api,
      baseUrl: p.baseUrl,
      model: p.model,
      contextTokens: p.contextTokens,
      maxOutputTokens: p.maxOutputTokens,
    };
    models = [];
    status = null;
  }

  function input(): ProviderInput {
    return {
      ...form,
      proxy: form.proxy?.trim() ? form.proxy.trim() : null,
      apiKey: clearKey ? "" : key.trim() ? key.trim() : form.id ? null : "",
      headers: form.headers.filter(([name]) => name.trim() !== ""),
    };
  }

  async function attempt<T>(action: () => Promise<T>): Promise<T | undefined> {
    busy = true;
    status = null;
    try {
      return await action();
    } catch (error) {
      status = { ok: false, text: aiErrorText(asCommandError(error)) };
      return undefined;
    } finally {
      busy = false;
    }
  }

  async function test(): Promise<boolean> {
    const done = await attempt(async () => {
      await api.testProvider(input());
      return true;
    });
    if (done) status = { ok: true, text: t("provider.testOk") };
    return done ?? false;
  }

  async function listModels() {
    const list = await attempt(() => api.listModels(input()));
    if (list) {
      models = list;
      status = { ok: true, text: t("provider.modelsFound", { n: list.length }) };
    }
  }

  async function save(): Promise<string | undefined> {
    const before = new Set(ai.settings?.providers.map((p) => p.id) ?? []);
    const settings = await attempt(() => api.saveProvider(input()));
    if (!settings) return undefined;
    ai.settings = settings;
    return form.id ?? settings.providers.find((p) => !before.has(p.id))?.id;
  }

  async function submit() {
    if (!ready || busy) return;
    if (compact && !(await test())) return;
    const id = await save();
    if (id) onSaved?.(id);
  }
</script>

{#snippet address()}
  <label class="field">
    <span>{t("provider.baseUrl")}</span>
    <input class="base-url" bind:value={form.baseUrl} maxlength="500" spellcheck="false" placeholder="https://…/v1" />
    <small>{t("provider.baseUrlHint")}</small>
  </label>
{/snippet}

<form
  class="provider-form"
  onsubmit={(event) => {
    event.preventDefault();
    void submit();
  }}
>
  <label class="field">
    <span>{t("provider.preset")}</span>
    <select class="preset" value={form.preset} onchange={(event) => choose((event.currentTarget as HTMLSelectElement).value)}>
      {#each PRESETS as p (p.id)}
        <option value={p.id}>{t(`preset.${p.id}` as "preset.custom")}</option>
      {/each}
    </select>
  </label>
  {#if !compact}
    <label class="field">
      <span>{t("provider.name")}</span>
      <input class="name" bind:value={form.name} maxlength="60" />
    </label>
  {/if}
  {#if showAddress}{@render address()}{/if}
  <label class="field">
    <span>{t("provider.model")}</span>
    <span class="row">
      <input class="model" bind:value={form.model} maxlength="200" spellcheck="false" list={listId} />
      <button type="button" class="link list-models" disabled={busy || !form.baseUrl.trim()} onclick={listModels}>
        {t("provider.listModels")}
      </button>
    </span>
    <datalist id={listId}>
      {#each models as model (model)}<option value={model}></option>{/each}
    </datalist>
  </label>
  <label class="field">
    <span>{t("provider.apiKey")}</span>
    <input
      class="api-key"
      type="password"
      autocomplete="off"
      spellcheck="false"
      maxlength="4096"
      bind:value={key}
      disabled={clearKey}
      placeholder={hasKey ? t("provider.keySaved") : local ? t("provider.keyOptional") : "sk-…"}
    />
  </label>
  {#if hasKey}
    <label class="check">
      <input type="checkbox" bind:checked={clearKey} />
      <span>{t("provider.clearKey")}</span>
    </label>
  {/if}

  <details>
    <summary>{t("provider.advanced")}</summary>
    {#if !showAddress}{@render address()}{/if}
    <label class="field">
      <span>{t("provider.api")}</span>
      <select bind:value={form.api}>
        <option value="openAi">{t("provider.apiOpenAi")}</option>
        <option value="anthropic">{t("provider.apiAnthropic")}</option>
      </select>
    </label>
    <label class="field">
      <span>{t("provider.context")}</span>
      <input type="number" min="2048" max="2000000" step="1024" bind:value={form.contextTokens} />
    </label>
    <label class="field">
      <span>{t("provider.output")}</span>
      <input type="number" min="256" max="128000" step="256" bind:value={form.maxOutputTokens} />
      <small>{t("provider.limitsHint")}</small>
    </label>
    <label class="field">
      <span>{t("provider.proxy")}</span>
      <input
        value={form.proxy ?? ""}
        oninput={(event) => (form.proxy = (event.currentTarget as HTMLInputElement).value)}
        maxlength="500"
        spellcheck="false"
        placeholder={t("provider.proxyHint")}
      />
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={form.allowHttp} />
      <span>{t("provider.allowHttp")}</span>
    </label>
    <div class="field">
      <span>{t("provider.headers")}</span>
      <div class="headers">
        {#each form.headers as header, index (index)}
          <span class="row">
            <input placeholder={t("provider.headerName")} bind:value={header[0]} maxlength="64" spellcheck="false" />
            <input
              type="password"
              autocomplete="off"
              placeholder={provider?.headerNames.includes(header[0]) ? t("provider.headerSaved") : t("provider.headerValue")}
              bind:value={header[1]}
              maxlength="4096"
            />
            <button type="button" class="link" onclick={() => (form.headers = form.headers.filter((_, i) => i !== index))}>
              {t("provider.removeHeader")}
            </button>
          </span>
        {/each}
        {#if form.headers.length < 10}
          <button type="button" class="link" onclick={() => (form.headers = [...form.headers, ["", ""]])}>
            {t("provider.addHeader")}
          </button>
        {/if}
      </div>
    </div>
  </details>

  <div class="buttons">
    <button type="submit" class="primary save-provider" disabled={!ready || busy}>
      {submitLabel ?? (compact ? t("ai.testAndStart") : t("provider.save"))}
    </button>
    {#if !compact}
      <button type="button" class="test-provider" disabled={!ready || busy} onclick={test}>{t("provider.test")}</button>
    {/if}
    {#if onCancel}
      <button type="button" class="link" onclick={onCancel}>{t("provider.cancel")}</button>
    {/if}
    {#if busy}
      <span class="muted">{t("provider.testing")}</span>
    {:else if status}
      <span class="status" class:ok={status.ok} class:bad={!status.ok}>{status.text}</span>
    {/if}
  </div>
</form>

<style>
  .provider-form {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .field {
    display: grid;
    grid-template-columns: 160px 1fr;
    gap: 4px 12px;
    align-items: center;
    margin: 4px 0;
  }
  .field small {
    grid-column: 2;
    color: var(--muted);
    font-size: 12px;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .row input {
    flex: 1;
    min-width: 0;
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 4px 0 4px 172px;
    font-size: 13px;
  }
  .headers {
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: flex-start;
  }
  .headers .row {
    width: 100%;
  }
  details {
    margin: 4px 0;
  }
  summary {
    cursor: pointer;
    color: var(--muted);
    font-size: 13px;
    margin-bottom: 4px;
  }
  .buttons {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-top: 8px;
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
  .status {
    font-size: 13px;
    word-break: break-word;
  }
  .status.ok {
    color: var(--ok);
  }
  .status.bad {
    color: var(--danger);
  }
</style>
