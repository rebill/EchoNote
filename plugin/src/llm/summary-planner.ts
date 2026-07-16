const MIN_BOUNDARY_RATIO = 0.6;

export function splitTextAtBoundaries(text: string, maxChars: number): string[] {
  if (!Number.isInteger(maxChars) || maxChars < 1) {
    throw new Error("maxChars must be a positive integer.");
  }
  if (!text) {
    return [];
  }

  const chunks: string[] = [];
  let start = 0;
  while (start < text.length) {
    const hardEnd = Math.min(text.length, start + maxChars);
    const end = hardEnd < text.length ? findBoundary(text, start, hardEnd) : hardEnd;
    chunks.push(text.slice(start, end));
    start = end;
  }
  return chunks;
}

export async function mapWithConcurrency<T, R>(
  items: T[],
  concurrency: number,
  operation: (item: T, index: number) => Promise<R>
): Promise<R[]> {
  const workerCount = Math.max(1, Math.min(items.length, Math.floor(concurrency)));
  const results = new Array<R>(items.length);
  let nextIndex = 0;

  async function worker(): Promise<void> {
    while (nextIndex < items.length) {
      const index = nextIndex;
      nextIndex += 1;
      results[index] = await operation(items[index], index);
    }
  }

  await Promise.all(Array.from({ length: workerCount }, () => worker()));
  return results;
}

export function groupByRenderedBudget<T>(
  items: T[],
  maxChars: number,
  renderItem: (item: T, index: number) => string,
  separator = "\n\n"
): T[][] {
  if (!Number.isInteger(maxChars) || maxChars < 1) {
    throw new Error("maxChars must be a positive integer.");
  }

  const groups: T[][] = [];
  let group: T[] = [];
  let groupLength = 0;

  for (let index = 0; index < items.length; index += 1) {
    const item = items[index];
    const itemLength = renderItem(item, index).length;
    if (itemLength > maxChars) {
      throw new Error(`A partial summary exceeds the merge budget of ${maxChars} characters.`);
    }

    const nextLength = group.length === 0 ? itemLength : groupLength + separator.length + itemLength;
    if (group.length > 0 && nextLength > maxChars) {
      groups.push(group);
      group = [];
      groupLength = 0;
    }

    group.push(item);
    groupLength = groupLength === 0 ? itemLength : groupLength + separator.length + itemLength;
  }

  if (group.length > 0) {
    groups.push(group);
  }
  return groups;
}

function findBoundary(text: string, start: number, hardEnd: number): number {
  const minimum = start + Math.floor((hardEnd - start) * MIN_BOUNDARY_RATIO);
  let sentenceBoundary = -1;

  for (let index = hardEnd - 1; index >= minimum; index -= 1) {
    const character = text[index];
    if (character === "\n") {
      return index + 1;
    }
    if (sentenceBoundary < 0 && "。！？.!?".includes(character)) {
      sentenceBoundary = index + 1;
    }
  }

  if (sentenceBoundary > start) {
    return sentenceBoundary;
  }
  if (isLowSurrogate(text.charCodeAt(hardEnd)) && isHighSurrogate(text.charCodeAt(hardEnd - 1))) {
    return hardEnd - 1;
  }
  return hardEnd;
}

function isHighSurrogate(value: number): boolean {
  return value >= 0xd800 && value <= 0xdbff;
}

function isLowSurrogate(value: number): boolean {
  return value >= 0xdc00 && value <= 0xdfff;
}
