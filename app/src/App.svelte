<script lang="ts">
  import { onMount } from "svelte";

  import MeetingDetail from "./components/MeetingDetail.svelte";
  import RecordingView from "./components/RecordingView.svelte";
  import SettingsView from "./components/SettingsView.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import { ai } from "./lib/ai.svelte";
  import { app } from "./lib/app.svelte";
  import { errorText, t } from "./lib/i18n.svelte";
  import { asCommandError } from "./lib/ipc";

  let ready = $state(false);

  onMount(() => {
    app
      .init()
      .then(async () => {
        // AI settings are not needed to show the app.
        await ai.init().catch((error) => app.showError(asCommandError(error)));
        ready = true;
        if (app.info?.smoke) (await import("./smoke")).run();
      })
      .catch((error) => {
        ready = true;
        app.toast = errorText(asCommandError(error));
      });
  });

  $effect(() => {
    if (!app.toast) return;
    const timer = setTimeout(() => (app.toast = null), 6000);
    return () => clearTimeout(timer);
  });

  const shortcut = $derived(app.settings?.shortcut ?? "");
</script>

{#if ready}
  <div class="shell">
    <Sidebar />
    <main>
      {#if app.info?.engineError}
        <div class="fatal">{errorText(app.info.engineError)}</div>
      {:else if app.view.name === "live"}
        <RecordingView />
      {:else if app.view.name === "meeting"}
        {#key app.view.id}
          <MeetingDetail id={app.view.id} focus={app.view.focus} />
        {/key}
      {:else if app.view.name === "settings"}
        <SettingsView />
      {:else}
        <div class="home">
          <h1>{t("empty.title")}</h1>
          <p>{shortcut ? t("empty.body", { shortcut }) : t("empty.bodyNoShortcut")}</p>
          <button class="primary" disabled={app.starting} onclick={() => app.startRecording()}>{t("nav.record")}</button>
        </div>
      {/if}
    </main>
  </div>
  {#if app.toast}
    <div class="toast" role="status">
      <span>{app.toast}</span>
      <button class="link" onclick={() => (app.toast = null)}>{t("notice.dismiss")}</button>
    </div>
  {/if}
{/if}

<style>
  .shell {
    display: grid;
    grid-template-columns: 260px 1fr;
    height: 100vh;
  }
  main {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .home {
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 12px;
    height: 100%;
    padding: 24px;
    text-align: center;
  }
  .home h1 {
    font-size: 22px;
    margin: 0;
  }
  .home p {
    max-width: 420px;
    color: var(--muted);
    margin: 0;
  }
  .primary {
    padding: 10px 20px;
    font-weight: 600;
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }
  .fatal {
    margin: 24px;
    padding: 12px 16px;
    border-radius: 6px;
    background: var(--error-bg);
  }
  .toast {
    position: fixed;
    right: 20px;
    bottom: 20px;
    max-width: 420px;
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 10px 14px;
    border-radius: 8px;
    background: var(--toast);
    color: var(--toast-text);
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.2);
  }
</style>
