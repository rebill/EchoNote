<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { t } from "../lib/i18n.svelte";
  import { api, asCommandError } from "../lib/ipc";
  import { splitAliases } from "../lib/live";
  import type { GlossaryEntry } from "../lib/types";

  let entries = $state.raw<GlossaryEntry[]>([]);
  let editing = $state<number | null>(null);
  let term = $state("");
  let aliases = $state("");

  $effect(() => {
    api
      .listGlossary()
      .then((list) => (entries = list))
      .catch((error) => app.showError(asCommandError(error)));
  });

  function edit(entry: GlossaryEntry) {
    editing = entry.id;
    term = entry.term;
    aliases = entry.aliases.join(", ");
  }

  function reset() {
    editing = null;
    term = "";
    aliases = "";
  }

  async function save() {
    if (!term.trim()) return;
    try {
      entries = await api.saveGlossaryEntry({ id: editing, term, aliases: splitAliases(aliases) });
      reset();
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }

  async function remove(id: number) {
    try {
      entries = await api.deleteGlossaryEntry(id);
      if (editing === id) reset();
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }
</script>

<p class="hint">{t("glossary.hint")}</p>
<form
  class="entry-form"
  onsubmit={(event) => {
    event.preventDefault();
    void save();
  }}
>
  <input class="term" placeholder={t("glossary.term")} bind:value={term} maxlength="60" spellcheck="false" />
  <input class="aliases" placeholder={t("glossary.aliases")} bind:value={aliases} maxlength="1200" spellcheck="false" />
  <button type="submit" disabled={!term.trim()}>{editing == null ? t("glossary.add") : t("glossary.save")}</button>
  {#if editing != null}
    <button type="button" class="link" onclick={reset}>{t("glossary.cancel")}</button>
  {/if}
</form>
{#if entries.length === 0}
  <p class="hint">{t("glossary.empty")}</p>
{:else}
  <ul class="entries">
    {#each entries as entry (entry.id)}
      <li class:active={editing === entry.id}>
        <span class="term">{entry.term}</span>
        <span class="aliases">{entry.aliases.join(", ")}</span>
        <button class="link" onclick={() => edit(entry)}>{t("edit.edit")}</button>
        <button class="link danger" onclick={() => remove(entry.id)}>{t("glossary.delete")}</button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .hint {
    margin: 4px 0 8px;
    font-size: 13px;
    color: var(--muted);
  }
  .entry-form {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .entry-form .term {
    width: 160px;
  }
  .entry-form .aliases {
    flex: 1;
  }
  .entries {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    border-top: 1px solid var(--border);
  }
  .entries li {
    display: grid;
    grid-template-columns: 160px 1fr auto auto;
    gap: 12px;
    align-items: baseline;
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }
  .entries li.active {
    background: var(--highlight);
  }
  .entries .term {
    font-weight: 600;
    word-break: break-word;
  }
  .entries .aliases {
    color: var(--muted);
    word-break: break-word;
  }
  .danger {
    color: var(--danger);
  }
</style>
