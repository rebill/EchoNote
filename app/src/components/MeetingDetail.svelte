<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { dateTime, duration } from "../lib/format";
  import { locale, t } from "../lib/i18n.svelte";
  import { api, asCommandError, audioUrl } from "../lib/ipc";
  import { hasOtherSpeech, segmentRows } from "../lib/live";
  import type { MeetingDetail } from "../lib/types";
  import TranscriptList from "./TranscriptList.svelte";

  let { id, focus = null }: { id: number; focus?: number | null } = $props();

  let detail = $state.raw<MeetingDetail | null>(null);
  let tab = $state<"summary" | "transcript" | "qa">("transcript");
  let showHidden = $state(false);
  let title = $state("");
  let editing = $state(false);

  $effect(() => {
    void app.libraryVersion;
    const current = id;
    api
      .getMeeting(current)
      .then((loaded) => {
        if (current === id) detail = loaded;
      })
      .catch((error) => app.showError(asCommandError(error)));
  });

  $effect(() => {
    void id;
    showHidden = false;
    tab = "transcript";
  });

  $effect(() => {
    if (!editing) title = detail?.meeting.title ?? "";
  });

  const rows = $derived(detail ? segmentRows(detail.segments, showHidden) : []);
  const hiddenCount = $derived(detail ? detail.segments.filter((s) => s.crosstalk === "duplicate").length : 0);
  const labels = $derived(detail ? hasOtherSpeech(detail.segments) : false);
  const recording = $derived(app.recording && app.live?.meeting === id);
  const hasAudio = $derived(detail ? detail.tracks.some((track) => (track.durationMs ?? 0) > 0) : false);
  let audio = $state<HTMLAudioElement>();
  let audioFailed = $state(false);
  const playable = $derived(hasAudio && !app.recording && !audioFailed);

  $effect(() => {
    void id;
    audioFailed = false;
  });

  // Seeking before the metadata arrives is ignored by some engines, so wait for it.
  function seek(startMs: number) {
    const player = audio;
    if (!player) return;
    const play = () => {
      player.currentTime = startMs / 1000;
      player.play().catch(() => {});
    };
    if (player.readyState >= HTMLMediaElement.HAVE_METADATA) play();
    else {
      player.addEventListener("loadedmetadata", play, { once: true });
      player.load();
    }
  }

  async function rename() {
    editing = false;
    if (!detail || title.trim() === detail.meeting.title) return;
    try {
      await api.renameMeeting(id, title);
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }
</script>

{#if detail}
  <section class="detail">
    <header>
      <input
        class="title"
        placeholder={app.title(detail.meeting)}
        bind:value={title}
        onfocus={() => (editing = true)}
        onblur={rename}
        onkeydown={(event) => event.key === "Enter" && (event.currentTarget as HTMLInputElement).blur()}
        maxlength="200"
      />
      <div class="meta">
        <span>{dateTime(detail.meeting.startedAtMs, locale.value)}</span>
        {#if detail.meeting.durationMs != null}
          <span>{t("detail.duration", { duration: duration(detail.meeting.durationMs, locale.value) })}</span>
        {/if}
        {#if detail.meeting.status === "recovered"}
          <span class="badge" title={t("status.recoveredHint")}>{t("status.recovered")}</span>
        {/if}
      </div>
    </header>

    {#if recording}
      <div class="banner">
        {t("detail.recording")}
        <button class="link" onclick={() => (app.view = { name: "live" })}>{t("detail.openLive")}</button>
      </div>
    {:else if app.recording && hasAudio}
      <!-- The system-audio tap would record the playback. -->
      <p class="player-note">{t("detail.playbackRecording")}</p>
    {:else if hasAudio && !audioFailed}
      {#key `${id}:${detail.meeting.durationMs}`}
        <audio
          class="player"
          controls
          preload="metadata"
          src={audioUrl(id)}
          bind:this={audio}
          onerror={() => (audioFailed = true)}
        ></audio>
      {/key}
    {:else if hasAudio}
      <p class="player-note">{t("detail.noAudio")}</p>
    {/if}

    <div class="tabs" role="tablist">
      {#each [["summary", "detail.summary"], ["transcript", "detail.transcript"], ["qa", "detail.qa"]] as const as [name, label] (name)}
        <button role="tab" aria-selected={tab === name} class:active={tab === name} onclick={() => (tab = name)}>
          {t(label)}
        </button>
      {/each}
      {#if tab === "transcript" && hiddenCount > 0}
        <button class="link toggle" onclick={() => (showHidden = !showHidden)}>
          {showHidden ? t("detail.hideHidden") : t("detail.showHidden", { n: hiddenCount })}
        </button>
      {/if}
    </div>

    <div class="body">
      {#if tab === "summary"}
        <p class="placeholder">{t("detail.summaryLater")}</p>
      {:else if tab === "qa"}
        <p class="placeholder">{t("detail.qaLater")}</p>
      {:else if rows.length === 0}
        <p class="placeholder">{t("detail.empty")}</p>
      {:else}
        <TranscriptList {rows} {labels} {focus} onSeek={playable ? seek : undefined} />
      {/if}
    </div>
  </section>
{/if}

<style>
  .detail {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  header {
    padding: 16px 20px 8px;
  }
  .title {
    width: 100%;
    font-size: 18px;
    font-weight: 600;
    border: 1px solid transparent;
    background: transparent;
  }
  .title:hover,
  .title:focus {
    border-color: var(--border);
  }
  .meta {
    display: flex;
    gap: 12px;
    margin-top: 4px;
    font-size: 13px;
    color: var(--muted);
  }
  .badge {
    padding: 0 6px;
    border-radius: 4px;
    background: var(--warn-bg);
    color: var(--text);
  }
  .banner {
    display: flex;
    gap: 12px;
    margin: 0 20px 8px;
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
    background: var(--info-bg);
  }
  .player {
    display: block;
    width: calc(100% - 40px);
    height: 36px;
    margin: 0 20px 8px;
  }
  .player-note {
    margin: 0 20px 8px;
    font-size: 13px;
    color: var(--muted);
  }
  .tabs {
    display: flex;
    gap: 4px;
    padding: 0 20px;
    border-bottom: 1px solid var(--border);
  }
  .tabs [role="tab"] {
    border: none;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    background: none;
    padding: 8px 12px;
  }
  .tabs [role="tab"].active {
    border-bottom-color: var(--accent);
    color: var(--accent);
  }
  .toggle {
    margin-left: auto;
  }
  .body {
    flex: 1;
    min-height: 0;
  }
  .placeholder {
    padding: 24px 20px;
    color: var(--muted);
  }
</style>
