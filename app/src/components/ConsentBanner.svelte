<script lang="ts">
  import { ai, type RunKind } from "../lib/ai.svelte";
  import { t } from "../lib/i18n.svelte";

  let { meeting, kinds }: { meeting: number; kinds: (RunKind | "highlights")[] } = $props();

  const consent = $derived(ai.consent && ai.consent.meeting === meeting && kinds.includes(ai.consent.kind) ? ai.consent : null);
</script>

{#if consent}
  <div class="consent" role="alertdialog" aria-label={t("ai.consentTitle")}>
    <strong>{t("ai.consentTitle")}</strong>
    <span>{t("ai.consentBody", { origin: consent.provider.origin ?? consent.provider.baseUrl })}</span>
    <span class="buttons">
      <button class="primary consent-yes" onclick={() => ai.agree()}>{t("ai.consentYes")}</button>
      <button class="link" onclick={() => (ai.consent = null)}>{t("ai.consentNo")}</button>
    </span>
  </div>
{/if}

<style>
  .consent {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 8px 0;
    padding: 10px 12px;
    border-radius: 6px;
    font-size: 13px;
    background: var(--warn-bg);
  }
  .buttons {
    display: flex;
    gap: 12px;
    align-items: center;
    margin-top: 4px;
  }
  .primary {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }
</style>
