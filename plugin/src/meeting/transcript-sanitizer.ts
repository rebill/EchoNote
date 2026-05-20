const MIN_REPEATED_UNIT_CHARS = 8;
const MAX_REPEATED_UNIT_CHARS = 120;
const MIN_REPETITIONS = 4;
const MAX_SEGMENT_CHARS = 3000;

export function sanitizeTranscriptText(text: string): string {
  const normalized = text.trim();
  if (!normalized) {
    return "";
  }

  const collapsed = collapseRepeatedRuns(collapseRepeatedSentences(normalized)).trim();
  if (collapsed.length <= MAX_SEGMENT_CHARS) {
    return collapsed;
  }

  return trimAtBoundary(collapsed, MAX_SEGMENT_CHARS);
}

function collapseRepeatedSentences(text: string): string {
  const sentences = splitSentences(text);
  if (sentences.length < MIN_REPETITIONS) {
    return text;
  }

  const output: string[] = [];
  let index = 0;
  while (index < sentences.length) {
    const sentence = sentences[index];
    let repetitions = 1;
    while (index + repetitions < sentences.length && sentences[index + repetitions] === sentence) {
      repetitions += 1;
    }

    index += repetitions;
    if (repetitions >= MIN_REPETITIONS) {
      output.push(sentence);
      if (index < sentences.length && sentence.startsWith(sentences[index])) {
        index += 1;
      }
      continue;
    }

    for (let offset = 0; offset < repetitions; offset += 1) {
      output.push(sentence);
    }
  }

  return output.join("");
}

function splitSentences(text: string): string[] {
  const sentences: string[] = [];
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    if (!"。！？.!?".includes(text[index])) {
      continue;
    }

    sentences.push(text.slice(start, index + 1));
    start = index + 1;
  }

  if (start < text.length) {
    sentences.push(text.slice(start));
  }

  return sentences;
}

function collapseRepeatedRuns(text: string): string {
  const output: string[] = [];
  let index = 0;

  while (index < text.length) {
    const match = findRepeatedRun(text, index);
    if (!match) {
      output.push(text[index]);
      index += 1;
      continue;
    }

    output.push(text.slice(index, index + match.unitLength));
    index += match.unitLength * match.repetitions;
  }

  return output.join("");
}

function findRepeatedRun(text: string, index: number): { unitLength: number; repetitions: number } | null {
  const remaining = text.length - index;
  const maxUnitLength = Math.min(MAX_REPEATED_UNIT_CHARS, Math.floor(remaining / MIN_REPETITIONS));

  for (let unitLength = MIN_REPEATED_UNIT_CHARS; unitLength <= maxUnitLength; unitLength += 1) {
    const unit = text.slice(index, index + unitLength);
    if (!unit.trim()) {
      continue;
    }

    let repetitions = 1;
    let cursor = index + unitLength;
    while (text.startsWith(unit, cursor)) {
      repetitions += 1;
      cursor += unitLength;
    }

    if (repetitions < MIN_REPETITIONS) {
      continue;
    }

    return { unitLength, repetitions };
  }

  return null;
}

function trimAtBoundary(text: string, maxChars: number): string {
  const candidate = text.slice(0, maxChars).trimEnd();
  for (const boundary of ["。", "！", "？", ".", "!", "?"]) {
    const position = candidate.lastIndexOf(boundary);
    if (position >= Math.floor(maxChars / 2)) {
      return candidate.slice(0, position + 1);
    }
  }
  return candidate;
}
