<script lang="ts">
  let { label, device, level, active }: { label: string; device: string; level: number; active: boolean } =
    $props();

  // -60 dBFS to 0 dBFS onto 0..1.
  const fill = $derived.by(() => {
    if (!active || level <= 0) return 0;
    const db = 20 * Math.log10(level);
    return Math.min(1, Math.max(0, (db + 60) / 60));
  });
</script>

<div class="meter" class:inactive={!active}>
  <div class="label">
    <span>{label}</span>
    <span class="device" title={device}>{device}</span>
  </div>
  <div class="bar"><div class="fill" style:transform={`scaleX(${fill})`}></div></div>
</div>

<style>
  .meter {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  .label {
    display: flex;
    gap: 8px;
    font-size: 12px;
    color: var(--muted);
  }
  .device {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--track);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transform-origin: left center;
    transition: transform 80ms linear;
  }
  .inactive .fill {
    background: var(--muted);
  }
</style>
