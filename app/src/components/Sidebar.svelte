<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { clock, duration, shortDate } from "../lib/format";
  import { locale, t } from "../lib/i18n.svelte";
  import { api } from "../lib/ipc";
  import { elapsed } from "../lib/live";
  import type { SearchHit } from "../lib/types";

  let query = $state("");
  let hits = $state.raw<SearchHit[]>([]);
  let now = $state(performance.now());

  $effect(() => {
    if (!app.recording) return;
    const timer = setInterval(() => (now = performance.now()), 500);
    return () => clearInterval(timer);
  });

  // Search the transcripts 250 ms after the last keystroke; drop answers to older queries.
  $effect(() => {
    const current = query.trim();
    void app.libraryVersion;
    if (!current) {
      hits = [];
      return;
    }
    const timer = setTimeout(() => {
      api
        .search(current)
        .then((found) => {
          if (query.trim() === current) hits = found;
        })
        .catch(() => (hits = []));
    }, 250);
    return () => clearTimeout(timer);
  });

  const searching = $derived(query.trim() !== "");
  const titleMatches = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    return needle ? app.meetings.filter((m) => app.title(m).toLowerCase().includes(needle)) : [];
  });
  const selected = $derived(app.view.name === "meeting" ? app.view.id : null);
  const recordingTime = $derived(app.live && app.recording ? clock(elapsed(app.live, now)) : "");
</script>

<aside>
  <div class="brand">EchoNote</div>
  <button class="record" class:on={app.recording} disabled={app.starting} onclick={() => app.startRecording()}>
    <span class="dot"></span>
    {#if app.recording}
      {t("nav.recording")} {recordingTime}
    {:else}
      {t("nav.record")}
    {/if}
  </button>

  <input class="search" type="search" placeholder={t("nav.search")} bind:value={query} maxlength="200" />

  <nav>
    {#if searching}
      {#if titleMatches.length === 0 && hits.length === 0}
        <p class="empty">{t("nav.noResults")}</p>
      {/if}
      {#if titleMatches.length > 0}
        <h3>{t("nav.titleMatches")}</h3>
        {#each titleMatches as meeting (meeting.id)}
          <button class="item" class:selected={selected === meeting.id} onclick={() => (app.view = { name: "meeting", id: meeting.id, focus: null })}>
            <span class="name">{app.title(meeting)}</span>
            <span class="sub">{shortDate(meeting.startedAtMs, locale.value)}</span>
          </button>
        {/each}
      {/if}
      {#if hits.length > 0}
        <h3>{t("nav.transcriptMatches")}</h3>
        {#each hits as hit (hit.segmentId)}
          <button class="item" onclick={() => (app.view = { name: "meeting", id: hit.meetingId, focus: hit.segmentId })}>
            <span class="name">{hit.text}</span>
            <span class="sub">{app.title({ title: hit.meetingTitle, startedAtMs: app.meetings.find((m) => m.id === hit.meetingId)?.startedAtMs ?? 0 })} · {clock(hit.startMs)}</span>
          </button>
        {/each}
      {/if}
    {:else if app.meetings.length === 0}
      <p class="empty">{t("nav.noMeetings")}</p>
    {:else}
      {#each app.meetings as meeting (meeting.id)}
        <button class="item" class:selected={selected === meeting.id} onclick={() => (app.view = { name: "meeting", id: meeting.id, focus: null })}>
          <span class="name">{app.title(meeting)}</span>
          <span class="sub">
            {shortDate(meeting.startedAtMs, locale.value)}
            {#if meeting.status === "recording"}
              · <span class="live-tag">{t("status.recording")}</span>
            {:else if meeting.durationMs != null}
              · {duration(meeting.durationMs, locale.value)}
            {/if}
            {#if meeting.status === "recovered"} · {t("status.recovered")}{/if}
          </span>
        </button>
      {/each}
    {/if}
  </nav>

  <button class="settings" class:selected={app.view.name === "settings"} onclick={() => (app.view = { name: "settings" })}>
    {t("nav.settings")}
  </button>
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 10px;
    background: var(--sidebar);
    border-right: 1px solid var(--border);
    min-height: 0;
  }
  .brand {
    font-weight: 700;
    padding: 0 6px;
  }
  .record {
    display: flex;
    gap: 8px;
    align-items: center;
    justify-content: center;
    padding: 10px;
    font-weight: 600;
    background: var(--accent);
    border-color: var(--accent);
    color: white;
    font-variant-numeric: tabular-nums;
  }
  .record .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: white;
  }
  .record.on {
    background: var(--danger);
    border-color: var(--danger);
  }
  .search {
    width: 100%;
  }
  nav {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  h3 {
    font-size: 11px;
    color: var(--muted);
    margin: 8px 6px 2px;
    text-transform: uppercase;
  }
  .item {
    display: grid;
    gap: 2px;
    text-align: left;
    border: none;
    background: none;
    padding: 6px 8px;
    border-radius: 6px;
  }
  .item:hover {
    background: var(--hover);
  }
  .item.selected,
  .settings.selected {
    background: var(--selected);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    font-size: 12px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .live-tag {
    color: var(--danger);
  }
  .empty {
    color: var(--muted);
    padding: 6px 8px;
    font-size: 13px;
  }
  .settings {
    border: none;
    background: none;
    text-align: left;
    padding: 6px 8px;
  }
</style>
