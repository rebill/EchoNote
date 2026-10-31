<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { clock } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { api, asCommandError } from "../lib/ipc";
  import { elapsed, hasOtherSpeech, isActive, liveRows } from "../lib/live";
  import { banners, deviceName } from "../lib/notices";
  import LevelMeter from "./LevelMeter.svelte";
  import TranscriptList from "./TranscriptList.svelte";

  const live = $derived(app.live);
  let now = $state(performance.now());
  let busy = $state(false);
  let title = $state("");
  let editing = $state(false);
  let dismissed = $state<string[]>([]);

  $effect(() => {
    if (live?.state !== "recording") return;
    const timer = setInterval(() => (now = performance.now()), 250);
    return () => clearInterval(timer);
  });

  const rows = $derived(live ? liveRows(live) : []);
  const labels = $derived(live ? hasOtherSpeech(live.segments, live.previews) : false);
  const time = $derived(live ? clock(elapsed(live, now)) : "0:00");
  const shown = $derived(
    live ? banners(live.notices, app.info?.modelInstalled ?? true).filter((b) => !dismissed.includes(b.key)) : [],
  );
  const mic = $derived(live ? deviceName(live.notices, "mic") : null);
  const sys = $derived(live ? deviceName(live.notices, "sys") : null);
  const meeting = $derived(live ? app.meetings.find((m) => m.id === live.meeting) : undefined);

  $effect(() => {
    if (!editing) title = meeting?.title ?? "";
  });

  async function rename() {
    editing = false;
    if (!live || title.trim() === (meeting?.title ?? "")) return;
    try {
      await api.renameMeeting(live.meeting, title);
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }

  async function run(action: () => Promise<void>) {
    busy = true;
    try {
      await action();
    } catch (error) {
      app.showError(asCommandError(error));
    } finally {
      busy = false;
    }
  }
</script>

{#if live && isActive(live)}
  <section class="live">
    <header>
      <input
        class="title"
        placeholder={meeting ? app.title(meeting) : t("live.titlePlaceholder")}
        bind:value={title}
        onfocus={() => (editing = true)}
        onblur={rename}
        onkeydown={(event) => event.key === "Enter" && (event.currentTarget as HTMLInputElement).blur()}
        maxlength="200"
      />
      <div class="status">
        <span class="dot" class:on={live.state === "recording"}></span>
        <span class="time">{time}</span>
        {#if live.state === "starting"}<span class="muted">{t("live.starting")}</span>{/if}
        {#if live.state === "paused"}<span class="muted">{t("live.paused")}</span>{/if}
        {#if live.state === "stopping"}<span class="muted">{t("live.stopping")}</span>{/if}
      </div>
      <div class="controls">
        {#if live.state === "paused"}
          <button disabled={busy} onclick={() => run(api.resumeRecording)}>{t("live.resume")}</button>
        {:else}
          <button disabled={busy || live.state !== "recording"} onclick={() => run(api.pauseRecording)}>
            {t("live.pause")}
          </button>
        {/if}
        <button class="stop" disabled={busy || live.state === "stopping"} onclick={() => run(api.stopRecording)}>
          {t("live.stop")}
        </button>
      </div>
    </header>

    <div class="meters">
      <LevelMeter label={t("live.mic")} device={mic ?? ""} level={live.levels[0]} active={mic != null} />
      <LevelMeter
        label={t("live.sys")}
        device={sys ?? t("live.notRecorded")}
        level={live.levels[1]}
        active={sys != null}
      />
    </div>

    {#each shown as banner (banner.key)}
      <div class="banner {banner.level}">
        <span>{t(banner.text, banner.params)}</span>
        {#if banner.settings}
          <button class="link" onclick={() => api.openPrivacySettings(banner.settings!)}>{t("notice.openSettings")}</button>
        {/if}
        <button class="link" onclick={() => (dismissed = [...dismissed, banner.key])}>{t("notice.dismiss")}</button>
      </div>
    {/each}

    <div class="transcript">
      {#if rows.length === 0}
        <p class="waiting">{t("live.waiting")}</p>
      {:else}
        <TranscriptList {rows} {labels} follow />
      {/if}
    </div>
  </section>
{:else}
  <p class="waiting">{t("live.starting")}</p>
{/if}

<style>
  .live {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px 20px 8px;
  }
  .title {
    flex: 1;
    min-width: 0;
    font-size: 18px;
    font-weight: 600;
    border: 1px solid transparent;
    background: transparent;
  }
  .title:hover,
  .title:focus {
    border-color: var(--border);
  }
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .time {
    font-variant-numeric: tabular-nums;
    font-size: 16px;
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.on {
    background: var(--danger);
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
  .controls {
    display: flex;
    gap: 8px;
  }
  .stop {
    background: var(--danger);
    border-color: var(--danger);
    color: white;
  }
  .meters {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 24px;
    padding: 4px 20px 12px;
  }
  .banner {
    display: flex;
    gap: 12px;
    align-items: center;
    margin: 0 20px 8px;
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
    background: var(--info-bg);
  }
  .banner span {
    flex: 1;
  }
  .banner.warn {
    background: var(--warn-bg);
  }
  .banner.error {
    background: var(--error-bg);
  }
  .transcript {
    flex: 1;
    min-height: 0;
    border-top: 1px solid var(--border);
  }
  .waiting {
    padding: 24px 20px;
    color: var(--muted);
  }
  .muted {
    color: var(--muted);
    font-size: 13px;
  }
</style>
