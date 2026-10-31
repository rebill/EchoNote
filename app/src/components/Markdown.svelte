<script lang="ts">
  import { t } from "../lib/i18n.svelte";
  import { parse, type Block, type Inline } from "../lib/markdown";

  let { text, onSeek }: { text: string; onSeek?: (ms: number) => void } = $props();

  const blocks = $derived(parse(text));
</script>

{#snippet inlines(nodes: Inline[])}
  {#each nodes as node, index (index)}
    {#if node.kind === "text"}{node.text}{:else if node.kind === "strong"}<strong>{@render inlines(node.children)}</strong
      >{:else if node.kind === "em"}<em>{@render inlines(node.children)}</em>{:else if node.kind === "code"}<code
        >{node.text}</code
      >{:else if onSeek}<button class="cite" title={t("detail.playFrom")} onclick={() => onSeek(node.ms)}
        >[{node.label}]</button
      >{:else}<span class="cite">[{node.label}]</span>{/if}
  {/each}
{/snippet}

{#snippet render(list: Block[])}
  {#each list as block, index (index)}
    {#if block.kind === "heading"}
      {#if block.level <= 2}
        <h3>{@render inlines(block.inline)}</h3>
      {:else}
        <h4>{@render inlines(block.inline)}</h4>
      {/if}
    {:else if block.kind === "paragraph"}
      <p>{@render inlines(block.inline)}</p>
    {:else if block.kind === "list"}
      {#if block.ordered}
        <ol>
          {#each block.items as item, i (i)}
            <li>{@render inlines(item.inline)}{@render render(item.children)}</li>
          {/each}
        </ol>
      {:else}
        <ul>
          {#each block.items as item, i (i)}
            <li class:task={item.checked != null}>
              {#if item.checked != null}<span class="box">{item.checked ? "☑" : "☐"}</span>{/if}
              {@render inlines(item.inline)}{@render render(item.children)}
            </li>
          {/each}
        </ul>
      {/if}
    {:else if block.kind === "code"}
      <pre><code>{block.text}</code></pre>
    {:else if block.kind === "quote"}
      <blockquote>{@render render(block.children)}</blockquote>
    {:else if block.kind === "table"}
      <div class="table">
        <table>
          <thead>
            <tr>
              {#each block.head as cell, i (i)}<th>{@render inlines(cell)}</th>{/each}
            </tr>
          </thead>
          <tbody>
            {#each block.rows as row, r (r)}
              <tr>
                {#each row as cell, i (i)}<td>{@render inlines(cell)}</td>{/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <hr />
    {/if}
  {/each}
{/snippet}

<div class="markdown">{@render render(blocks)}</div>

<style>
  .markdown {
    line-height: 1.7;
    user-select: text;
    -webkit-user-select: text;
    word-break: break-word;
  }
  .markdown :global(h3) {
    font-size: 15px;
    margin: 18px 0 6px;
  }
  .markdown :global(h4) {
    font-size: 14px;
    margin: 14px 0 4px;
  }
  .markdown :global(p) {
    margin: 6px 0;
    white-space: pre-line;
  }
  .markdown :global(ul),
  .markdown :global(ol) {
    margin: 4px 0;
    padding-left: 22px;
  }
  .markdown :global(li) {
    margin: 2px 0;
    white-space: pre-line;
  }
  .markdown :global(li.task) {
    list-style: none;
    margin-left: -18px;
  }
  .markdown :global(.box) {
    margin-right: 4px;
  }
  .markdown :global(code) {
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 12.5px;
    padding: 0 3px;
    border-radius: 3px;
    background: var(--track);
  }
  .markdown :global(pre) {
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--track);
    overflow-x: auto;
  }
  .markdown :global(pre code) {
    padding: 0;
    background: none;
  }
  .markdown :global(blockquote) {
    margin: 6px 0;
    padding-left: 10px;
    border-left: 3px solid var(--border);
    color: var(--muted);
  }
  .markdown :global(.table) {
    overflow-x: auto;
  }
  .markdown :global(table) {
    border-collapse: collapse;
    margin: 6px 0;
  }
  .markdown :global(th),
  .markdown :global(td) {
    border: 1px solid var(--border);
    padding: 4px 8px;
    text-align: left;
  }
  .markdown :global(hr) {
    border: none;
    border-top: 1px solid var(--border);
  }
  .cite {
    font-variant-numeric: tabular-nums;
    font-size: 12.5px;
    color: var(--accent);
  }
  button.cite {
    padding: 0 1px;
    border: none;
    background: none;
    cursor: pointer;
  }
  button.cite:hover {
    text-decoration: underline;
  }
</style>
