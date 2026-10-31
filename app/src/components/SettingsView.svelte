<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { errorText, locale, t } from "../lib/i18n.svelte";
  import { bytes } from "../lib/format";
  import { api, asCommandError } from "../lib/ipc";
  import type { Device, Language, Permissions, Settings, StorageUsage } from "../lib/types";
  import AiSettings from "./AiSettings.svelte";
  import GlossaryEditor from "./GlossaryEditor.svelte";
  import ModelSettings from "./ModelSettings.svelte";

  const AUTO_STOP = [0, 5, 15, 30, 60];

  let devices = $state<Device[]>([]);
  let permissions = $state<Permissions | null>(null);
  let storage = $state<StorageUsage | null>(null);
  let shortcut = $state(app.settings?.shortcut ?? "");
  let shortcutStatus = $state<string | null>(app.info?.shortcutError ?? null);
  const settings = $derived(app.settings);
  const platform = $derived(app.info?.platform ?? "other");

  async function refresh() {
    permissions = await api.permissions().catch(() => null);
    devices = await api.listInputDevices().catch(() => []);
    storage = await api.storageUsage().catch(() => null);
  }

  $effect(() => {
    void refresh();
    // Permissions change in System Settings while the app is in the background.
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

  async function save(patch: Partial<Settings>): Promise<boolean> {
    if (!app.settings) return false;
    const next = { ...app.settings, ...patch };
    try {
      await api.setSettings(next);
      app.settings = next;
      locale.value = next.language;
      return true;
    } catch (error) {
      const failure = asCommandError(error);
      if ("shortcut" in patch) shortcutStatus = errorText(failure);
      else app.showError(failure);
      return false;
    }
  }

  async function saveShortcut() {
    const value = shortcut.trim();
    if (await save({ shortcut: value })) shortcutStatus = t("settings.saved");
  }
</script>

{#if settings}
  <section class="settings">
    <h1>{t("settings.title")}</h1>

    <h2>{t("settings.general")}</h2>
    <label class="field">
      <span>{t("settings.language")}</span>
      <select
        value={settings.language}
        onchange={(event) => save({ language: (event.currentTarget as HTMLSelectElement).value as Language })}
      >
        <option value="zh-CN">简体中文</option>
        <option value="en">English</option>
      </select>
    </label>

    <h2>{t("settings.recording")}</h2>
    <label class="field">
      <span>{t("settings.mic")}</span>
      <select
        value={settings.micDevice ?? ""}
        onchange={(event) => save({ micDevice: (event.currentTarget as HTMLSelectElement).value || null })}
      >
        <option value="">{t("settings.micDefault")}</option>
        {#each devices as device (device.id)}
          <option value={device.id}>{device.name}</option>
        {/each}
      </select>
    </label>
    <label class="check">
      <input
        type="checkbox"
        checked={settings.recordSystem}
        onchange={(event) => save({ recordSystem: (event.currentTarget as HTMLInputElement).checked })}
      />
      <span>{t("settings.recordSystem")}</span>
    </label>
    <label class="field">
      <span>{t("settings.autoStop")}</span>
      <select
        value={settings.autoStopMinutes}
        onchange={(event) => save({ autoStopMinutes: Number((event.currentTarget as HTMLSelectElement).value) })}
      >
        {#each AUTO_STOP as minutes (minutes)}
          <option value={minutes}>{minutes === 0 ? t("settings.never") : t("settings.minutes", { n: minutes })}</option>
        {/each}
      </select>
    </label>
    <div class="field">
      <span>{t("settings.shortcut")}</span>
      <div class="row">
        <input bind:value={shortcut} maxlength="64" spellcheck="false" />
        <button onclick={saveShortcut}>{t("settings.save")}</button>
      </div>
      <small>{shortcutStatus ?? t("settings.shortcutHint")}</small>
    </div>

    <h2>{t("models.title")}</h2>
    <ModelSettings />

    <h2>{t("glossary.title")}</h2>
    <GlossaryEditor />

    <h2>{t("settings.ai")}</h2>
    <AiSettings />

    <h2>{t("settings.permissions")}</h2>
    {#if platform === "macos" && permissions}
      {#each [["mic", "settings.permMic"], ["sys", "settings.permSys"]] as const as [kind, label] (kind)}
        <div class="field perm">
          <span>{t(label)}</span>
          <span class="state {permissions[kind]}">{t(`perm.${permissions[kind]}`)}</span>
          <button class="link" onclick={() => api.openPrivacySettings(kind)}>{t("settings.open")}</button>
        </div>
      {/each}
    {:else if platform === "windows" && permissions}
      <div class="field perm">
        <span>{t("settings.permMic")}</span>
        <span class="state {permissions.mic}">{t(`perm.${permissions.mic}`)}</span>
        <button class="link" onclick={() => api.openPrivacySettings("mic")}>{t("settings.open")}</button>
      </div>
      <small>{t("settings.windowsMic")}</small>
    {/if}

    {#if storage}
      <h2>{t("settings.storage")}</h2>
      <dl class="storage">
        <dt>{t("settings.storageAudio")}</dt>
        <dd>{bytes(storage.audioBytes)}</dd>
        <dt>{t("settings.storageLibrary")}</dt>
        <dd>{bytes(storage.libraryBytes)}</dd>
      </dl>
    {/if}

    <h2>{t("settings.about")}</h2>
    <dl>
      <dt>{t("settings.version")}</dt>
      <dd>{app.info?.version}</dd>
      <dt>{t("settings.model")}</dt>
      <dd>{app.info?.modelInstalled ? t("settings.installed") : t("settings.missing")}</dd>
      <dt>{t("settings.dataDir")}</dt>
      <dd class="path">{app.info?.dataDir}</dd>
    </dl>
  </section>
{/if}

<style>
  .settings {
    max-width: 640px;
    padding: 20px 24px 40px;
    overflow-y: auto;
    height: 100%;
  }
  h1 {
    font-size: 20px;
    margin: 0 0 8px;
  }
  h2 {
    font-size: 14px;
    margin: 24px 0 8px;
    color: var(--muted);
  }
  .field {
    display: grid;
    grid-template-columns: 200px 1fr;
    gap: 6px 12px;
    align-items: center;
    margin: 8px 0;
  }
  .field small {
    grid-column: 2;
    color: var(--muted);
  }
  .perm {
    grid-template-columns: 200px auto 1fr;
  }
  .perm .link {
    justify-self: start;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .row input {
    flex: 1;
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 8px 0;
  }
  .state.granted {
    color: var(--ok);
  }
  .state.denied {
    color: var(--danger);
  }
  dl {
    display: grid;
    grid-template-columns: 200px 1fr;
    gap: 6px 12px;
    margin: 0;
  }
  dd {
    margin: 0;
  }
  .path {
    user-select: text;
    word-break: break-all;
    font-size: 13px;
  }
  small {
    color: var(--muted);
  }
</style>
