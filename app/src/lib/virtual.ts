// Layout for a virtualized list of rows with different heights: measured heights by row key,
// an estimate for rows not measured yet, and the range of rows to render.

export class Layout {
  private heights = new Map<string, number>();

  constructor(readonly estimate = 56) {}

  /// Record a measured height; returns whether it changed.
  set(key: string, height: number): boolean {
    const rounded = Math.round(height);
    if (rounded <= 0 || this.heights.get(key) === rounded) return false;
    this.heights.set(key, rounded);
    return true;
  }

  height(key: string): number {
    return this.heights.get(key) ?? this.estimate;
  }

  /// Top offset of every row; the last entry is the total height.
  offsets(keys: readonly string[]): number[] {
    const offsets = new Array<number>(keys.length + 1);
    offsets[0] = 0;
    for (let i = 0; i < keys.length; i++) offsets[i + 1] = offsets[i]! + this.height(keys[i]!);
    return offsets;
  }

  /// Forget rows that are gone.
  retain(keys: readonly string[]): void {
    const keep = new Set(keys);
    for (const key of this.heights.keys()) if (!keep.has(key)) this.heights.delete(key);
  }
}

/// First row whose bottom is below `y`.
export function rowAt(offsets: readonly number[], y: number): number {
  let low = 0;
  let high = offsets.length - 1;
  while (low < high) {
    const mid = (low + high) >> 1;
    if (offsets[mid + 1]! > y) high = mid;
    else low = mid + 1;
  }
  return Math.min(low, Math.max(0, offsets.length - 2));
}

/// Rows `[start, end)` that intersect the viewport, plus `overscan` on each side.
export function visibleRange(
  offsets: readonly number[],
  scrollTop: number,
  viewport: number,
  overscan = 6,
): [number, number] {
  const count = offsets.length - 1;
  if (count <= 0) return [0, 0];
  const first = rowAt(offsets, scrollTop);
  let last = first;
  while (last < count && offsets[last]! < scrollTop + viewport) last++;
  return [Math.max(0, first - overscan), Math.min(count, last + overscan)];
}
