<script lang="ts">
  import { clock } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import type { TranscriptRow } from "../lib/types";
  import { Layout, visibleRange } from "../lib/virtual";

  let {
    rows,
    labels,
    follow = false,
    focus = null,
    onSeek,
    onEdit,
    onRevert,
    onRestore,
  }: {
    rows: TranscriptRow[];
    /// Show "我"/"对方" (only when the other side spoke).
    labels: boolean;
    /// Stick to the bottom while the user has not scrolled up (live view).
    follow?: boolean;
    /// Scroll to this segment once.
    focus?: number | null;
    /// Makes the timestamps buttons that play from there.
    onSeek?: (startMs: number) => void;
    /// Editing actions; rows are read-only without them.
    onEdit?: (segmentId: number, text: string) => Promise<void>;
    onRevert?: (segmentId: number) => void;
    onRestore?: (segmentId: number) => void;
  } = $props();

  // Kept here, not in the row, so a draft survives the row scrolling out of view.
  let editing = $state<number | null>(null);
  let draft = $state("");

  function startEdit(row: TranscriptRow) {
    if (!onEdit || row.segmentId == null || row.preview) return;
    editing = row.segmentId;
    draft = row.text;
  }

  async function finishEdit(save: boolean) {
    const id = editing;
    if (id == null) return;
    editing = null;
    const original = rows.find((row) => row.segmentId === id)?.text;
    if (save && onEdit && draft.trim() && draft.trim() !== original) await onEdit(id, draft);
  }

  function onEditKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      void finishEdit(false);
    } else if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      void finishEdit(true);
    }
  }

  function autofocus(node: HTMLTextAreaElement) {
    node.focus();
    node.setSelectionRange(node.value.length, node.value.length);
  }

  const layout = new Layout(52);
  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(600);
  // Bumped when a measured height changes; `offsets` depends on it.
  let measured = $state(0);
  let atBottom = true;
  let focused: number | null = null;

  const keys = $derived(rows.map((row) => row.key));
  const offsets = $derived.by(() => {
    void measured;
    return layout.offsets(keys);
  });
  const total = $derived(offsets[offsets.length - 1] ?? 0);
  const range = $derived(visibleRange(offsets, scrollTop, height));
  const visible = $derived(rows.slice(range[0], range[1]));

  const observer = new ResizeObserver((entries) => {
    let changed = false;
    for (const entry of entries) {
      const key = (entry.target as HTMLElement).dataset.key;
      if (key && layout.set(key, entry.borderBoxSize[0]?.blockSize ?? entry.contentRect.height)) changed = true;
    }
    if (changed) measured++;
  });

  function measure(node: HTMLElement) {
    observer.observe(node);
    return { destroy: () => observer.unobserve(node) };
  }

  function onScroll() {
    if (!viewport) return;
    scrollTop = viewport.scrollTop;
    atBottom = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < 40;
  }

  $effect(() => {
    if (!viewport) return;
    const resize = new ResizeObserver(() => (height = viewport?.clientHeight ?? height));
    resize.observe(viewport);
    return () => resize.disconnect();
  });

  $effect(() => () => observer.disconnect());

  // Follow new lines in the live view.
  $effect(() => {
    void total;
    if (follow && atBottom && viewport) viewport.scrollTop = viewport.scrollHeight;
  });

  // Jump to a search hit once.
  $effect(() => {
    if (focus == null || focus === focused || !viewport) return;
    const index = rows.findIndex((row) => row.segmentId === focus);
    if (index < 0) return;
    focused = focus;
    viewport.scrollTop = Math.max(0, (offsets[index] ?? 0) - height / 3);
  });

  $effect(() => layout.retain(keys));
</script>

<div class="viewport" bind:this={viewport} onscroll={onScroll}>
  <div class="space" style:height={`${total}px`}>
    <div class="rows" style:transform={`translateY(${offsets[range[0]] ?? 0}px)`}>
      {#each visible as row (row.key)}
        <div
          class="row"
          class:preview={row.preview}
          class:hidden-line={row.hidden}
          class:focused={row.segmentId != null && row.segmentId === focus}
          data-key={row.key}
          use:measure
        >
          {#if onSeek && !row.preview}
            <button class="time seek" title={t("detail.playFrom")} onclick={() => onSeek(row.startMs)}>
              {clock(row.startMs)}
            </button>
          {:else}
            <span class="time">{clock(row.startMs)}</span>
          {/if}
          {#if labels}
            <span class="speaker" class:other={row.track === "sys"}>
              {row.track === "sys" ? t("live.other") : t("live.me")}
            </span>
          {/if}
          {#if editing != null && row.segmentId === editing}
            <textarea
              class="text editor"
              rows={Math.min(8, Math.max(2, Math.ceil(draft.length / 40)))}
              maxlength="3000"
              aria-label={t("edit.label")}
              bind:value={draft}
              onkeydown={onEditKey}
              onblur={() => finishEdit(true)}
              use:autofocus
            ></textarea>
          {:else}
            <span class="text" role="presentation" ondblclick={() => startEdit(row)}>
              {row.text}
              {#if row.hidden}<span class="tag">{t("detail.bleedTag")}</span>{/if}
              {#if row.edited}<span class="tag">{t("edit.editedTag")}</span>{/if}
            </span>
          {/if}
          {#if onEdit && row.segmentId != null && !row.preview && editing !== row.segmentId}
            {@const id = row.segmentId}
            <span class="actions">
              <button class="link edit" onclick={() => startEdit(row)}>{t("edit.edit")}</button>
              {#if row.edited && onRevert}
                <button class="link revert" title={t("edit.revertHint")} onclick={() => onRevert(id)}>{t("edit.revert")}</button>
              {/if}
              {#if (row.hidden || row.trimmed) && onRestore}
                <button class="link restore" title={t("edit.restoreHint")} onclick={() => onRestore(id)}>
                  {row.hidden ? t("edit.show") : t("edit.untrim")}
                </button>
              {/if}
            </span>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .viewport {
    height: 100%;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .space {
    position: relative;
  }
  .rows {
    position: absolute;
    inset: 0 0 auto 0;
    will-change: transform;
  }
  .row {
    display: grid;
    grid-template-columns: 56px auto 1fr auto;
    gap: 10px;
    align-items: baseline;
    padding: 8px 20px;
    line-height: 1.6;
  }
  .row.focused {
    background: var(--highlight);
  }
  .time {
    font-variant-numeric: tabular-nums;
    font-size: 12px;
    color: var(--muted);
  }
  .seek {
    padding: 0;
    border: none;
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .seek:hover,
  .seek:focus-visible {
    color: var(--accent);
    text-decoration: underline;
  }
  .speaker {
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
    white-space: nowrap;
  }
  .speaker.other {
    color: var(--other);
  }
  .text {
    grid-column: 3;
    word-break: break-word;
    user-select: text;
  }
  .row:not(:has(.speaker)) .text {
    grid-column: 2 / 4;
  }
  .editor {
    width: 100%;
    resize: vertical;
    font: inherit;
    line-height: 1.6;
    user-select: text;
  }
  .actions {
    grid-column: 4;
    display: flex;
    gap: 8px;
    font-size: 12px;
    white-space: nowrap;
    opacity: 0;
  }
  .row:hover .actions,
  .actions:focus-within {
    opacity: 1;
  }
  .preview .text {
    color: var(--muted);
  }
  .hidden-line .text {
    color: var(--muted);
    text-decoration: line-through;
  }
  .tag {
    margin-left: 6px;
    padding: 0 6px;
    border-radius: 4px;
    font-size: 11px;
    background: var(--track);
    text-decoration: none;
    display: inline-block;
  }
</style>
