// What an LLM correction changed in a line: a diff by word for Latin text and by character
// for CJK, which has no spaces.

export interface Part {
  kind: "same" | "add" | "del";
  text: string;
}

/// Longest inputs diffed in detail (tokens multiplied); past it the line is replaced whole.
const MAX_CELLS = 250_000;

/// Words, numbers and runs of spaces stay whole; every other character is its own token.
export function tokens(text: string): string[] {
  return text.match(/[A-Za-z0-9_'’-]+|\s+|./gsu) ?? [];
}

export function diff(before: string, after: string): Part[] {
  const a = tokens(before);
  const b = tokens(after);
  if (before === after) return before ? [{ kind: "same", text: before }] : [];
  if (a.length * b.length > MAX_CELLS) {
    return merge([
      { kind: "del", text: before },
      { kind: "add", text: after },
    ]);
  }
  // Longest common subsequence lengths from the end, then a forward walk.
  const width = b.length + 1;
  const table = new Uint32Array((a.length + 1) * width);
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      table[i * width + j] =
        a[i] === b[j]
          ? table[(i + 1) * width + j + 1]! + 1
          : Math.max(table[(i + 1) * width + j]!, table[i * width + j + 1]!);
    }
  }
  const parts: Part[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      parts.push({ kind: "same", text: a[i]! });
      i++;
      j++;
    } else if (table[(i + 1) * width + j]! >= table[i * width + j + 1]!) {
      parts.push({ kind: "del", text: a[i++]! });
    } else {
      parts.push({ kind: "add", text: b[j++]! });
    }
  }
  while (i < a.length) parts.push({ kind: "del", text: a[i++]! });
  while (j < b.length) parts.push({ kind: "add", text: b[j++]! });
  return merge(parts);
}

/// Each run of changes as one deletion then one addition, between unchanged text.
function merge(parts: Part[]): Part[] {
  const out: Part[] = [];
  let del = "";
  let add = "";
  const flush = () => {
    if (del) out.push({ kind: "del", text: del });
    if (add) out.push({ kind: "add", text: add });
    del = add = "";
  };
  for (const part of parts) {
    if (part.kind === "del") del += part.text;
    else if (part.kind === "add") add += part.text;
    else {
      flush();
      const last = out[out.length - 1];
      if (last?.kind === "same") last.text += part.text;
      else out.push({ ...part });
    }
  }
  flush();
  return out;
}
