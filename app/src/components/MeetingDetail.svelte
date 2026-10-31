<script lang="ts">
  import { ai, aiErrorText } from "../lib/ai.svelte";
  import { app } from "../lib/app.svelte";
  import { bytes, dateTime, duration, utcOffsetMinutes } from "../lib/format";
  import { locale, t } from "../lib/i18n.svelte";
  import { api, asCommandError, audioUrl } from "../lib/ipc";
  import { hasOtherSpeech, replaceSegment, segmentRows } from "../lib/live";
  import type { MeetingDetail, Segment } from "../lib/types";
  import ConsentBanner from "./ConsentBanner.svelte";
  import QaPanel from "./QaPanel.svelte";
  import SummaryPanel from "./SummaryPanel.svelte";
  import TranscriptList from "./TranscriptList.svelte";

  let { id, focus = null }: { id: number; focus?: number | null } = $props();

  let detail = $state.raw<MeetingDetail | null>(null);
  let tab = $state<"summary" | "transcript" | "qa">("transcript");
  let showHidden = $state(false);
  let title = $state("");
  let editing = $state(false);
  let confirmDelete = $state(false);
  let busy = $state(false);

  $effect(() => {
    void app.libraryVersion;
    const current = id;
    api
      .getMeeting(current)
      .then((loaded) => {
        if (current === id) detail = loaded;
      })
      .catch((error) => {
        // The refetch after this meeting was deleted.
        if (current === id && !(busy && asCommandError(error).code === "not-found")) {
          app.showError(asCommandError(error));
        }
      });
  });

  $effect(() => {
    void id;
    showHidden = false;
    tab = "transcript";
    confirmDelete = false;
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

  // The library-changed event refetches too; patching here keeps the row from flickering.
  async function change(action: Promise<Segment>) {
    try {
      const updated = await action;
      if (detail) detail = { ...detail, segments: replaceSegment(detail.segments, updated) };
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }

  const corrections = $derived(
    new Map((detail?.corrections ?? []).map((correction) => [correction.segmentId, correction.previousText])),
  );
  const correcting = $derived(ai.run(id, "correct"));

  function correct() {
    ai.start(id, "correct", () => api.correctMeeting(id));
  }

  async function keep(segment: number | null) {
    try {
      await api.keepCorrections(id, segment);
      app.libraryVersion++;
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }

  async function exportAs(format: "markdown" | "text") {
    if (!detail || busy) return;
    busy = true;
    try {
      const path = await api.exportMeeting(id, format, utcOffsetMinutes(detail.meeting.startedAtMs));
      if (path) app.toast = t("detail.exported", { path });
    } catch (error) {
      app.showError(asCommandError(error));
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (busy) return;
    busy = true;
    try {
      await api.deleteMeeting(id);
      app.view = { name: "home" };
      await app.loadMeetings();
    } catch (error) {
      app.showError(asCommandError(error));
    } finally {
      busy = false;
      confirmDelete = false;
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
        {#if detail.audioBytes > 0}
          <span>{t("detail.audioSize", { size: bytes(detail.audioBytes) })}</span>
        {/if}
        {#if detail.meeting.status === "recovered"}
          <span class="badge" title={t("status.recoveredHint")}>{t("status.recovered")}</span>
        {/if}
        <span class="actions">
          <span class="muted">{t("detail.export")}</span>
          <button class="link export-md" disabled={busy} onclick={() => exportAs("markdown")}>
            {t("detail.exportMarkdown")}
          </button>
          <button class="link export-txt" disabled={busy} onclick={() => exportAs("text")}>
            {t("detail.exportText")}
          </button>
          {#if !recording}
            <button class="link danger delete" disabled={busy} onclick={() => (confirmDelete = true)}>
              {t("detail.delete")}
            </button>
          {/if}
        </span>
      </div>
    </header>

    {#if confirmDelete && !recording}
      <div class="banner confirm" role="alertdialog" aria-label={t("detail.delete")}>
        <span>{t("detail.deleteConfirm")}</span>
        <button class="danger confirm-delete" disabled={busy} onclick={remove}>{t("detail.deleteYes")}</button>
        <button class="link" onclick={() => (confirmDelete = false)}>{t("detail.cancel")}</button>
      </div>
    {/if}

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
        {#if recording}
          <p class="placeholder">{t("detail.recording")}</p>
        {:else}
          <SummaryPanel {detail} onSeek={playable ? seek : undefined} />
        {/if}
      {:else if tab === "qa"}
        {#if recording}
          <p class="placeholder">{t("detail.recording")}</p>
        {:else}
          <QaPanel {detail} onSeek={playable ? seek : undefined} />
        {/if}
      {:else if rows.length === 0}
        <p class="placeholder">{t("detail.empty")}</p>
      {:else}
        <div class="transcript-tab">
          {#if !recording && ai.active}
            <div class="ai-bar">
              {#if correcting?.status === "running"}
                <button class="stop-correct" disabled={correcting.job == null} onclick={() => ai.cancel(id, "correct")}>
                  {t("correct.stop")}
                </button>
                <span class="muted">
                  {correcting.stage?.stage === "batches"
                    ? t("correct.progress", { done: correcting.stage.done, total: correcting.stage.total })
                    : t("correct.starting")}
                </span>
              {:else}
                <button class="link run-correct" onclick={correct}>{t("correct.run")}</button>
              {/if}
              {#if corrections.size > 0}
                <span class="pending">{t("correct.pending", { n: corrections.size })}</span>
                <button class="link keep-all" onclick={() => keep(null)}>{t("correct.keepAll")}</button>
              {/if}
              {#if correcting?.status === "error" && correcting.error}
                <span class="bad">{aiErrorText(correcting.error)}</span>
              {/if}
            </div>
            <ConsentBanner meeting={id} kinds={["correct"]} />
          {/if}
          <div class="list">
            <TranscriptList
              {rows}
              {labels}
              {focus}
              {corrections}
              onSeek={playable ? seek : undefined}
              onEdit={recording ? undefined : (segment, text) => change(api.editSegment(segment, text))}
              onRevert={recording ? undefined : (segment) => change(api.revertSegment(segment))}
              onRestore={recording ? undefined : (segment) => change(api.restoreSegment(segment))}
              onKeep={recording ? undefined : (segment) => keep(segment)}
              onUndo={recording ? undefined : (segment) => change(api.revertCorrection(segment))}
            />
          </div>
        </div>
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
  .actions {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
  .muted {
    color: var(--muted);
  }
  .danger {
    color: var(--danger);
  }
  .confirm {
    align-items: center;
    background: var(--warn-bg);
  }
  .confirm span {
    flex: 1;
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
  .transcript-tab {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .transcript-tab :global(.consent) {
    margin: 0 20px 8px;
  }
  .ai-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
    padding: 8px 20px;
    font-size: 13px;
    border-bottom: 1px solid var(--border);
  }
  .ai-bar .pending {
    margin-left: auto;
  }
  .ai-bar .bad {
    color: var(--danger);
  }
  .list {
    flex: 1;
    min-height: 0;
  }
</style>
