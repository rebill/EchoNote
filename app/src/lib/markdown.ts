// The Markdown subset model answers use, parsed to a tree that components render as
// elements. Nothing becomes HTML: links show their text only, images their alt text, raw
// HTML stays literal, so a transcript cannot make the UI load or open anything. `[mm:ss]`
// and `[h:mm:ss]` become time citations.

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "strong"; children: Inline[] }
  | { kind: "em"; children: Inline[] }
  | { kind: "code"; text: string }
  | { kind: "time"; ms: number; label: string };

export interface Item {
  inline: Inline[];
  /// `- [ ]` / `- [x]` items.
  checked: boolean | null;
  children: Block[];
}

export type Block =
  | { kind: "heading"; level: number; inline: Inline[] }
  | { kind: "paragraph"; inline: Inline[] }
  | { kind: "list"; ordered: boolean; items: Item[] }
  | { kind: "code"; text: string }
  | { kind: "quote"; children: Block[] }
  | { kind: "table"; head: Inline[][]; rows: Inline[][][] }
  | { kind: "rule" };

const HEADING = /^(#{1,6})\s+(.*?)\s*#*\s*$/;
const FENCE = /^\s*(```|~~~)/;
const RULE = /^\s*([-*_])(\s*\1){2,}\s*$/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const QUOTE = /^\s*>\s?(.*)$/;
const TABLE_RULE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
const TASK = /^\[([ xX])\]\s+(.*)$/;
const MAX_DEPTH = 8;

/// Seconds-precision citation: [12:34], [1:02:03], or several in one bracket [1:00, 2:30].
const TIME = /^\d{1,3}(:\d{2}){1,2}$/;

export function timeMs(label: string): number | null {
  if (!TIME.test(label)) return null;
  const parts = label.split(":").map(Number);
  if (parts.slice(1).some((part) => part >= 60)) return null;
  const [h = 0, m = 0, s = 0] = parts.length === 3 ? parts : [0, ...parts];
  return ((h * 60 + m) * 60 + s) * 1000;
}

function cells(line: string): string[] {
  let row = line.trim();
  if (row.startsWith("|")) row = row.slice(1);
  if (row.endsWith("|") && !row.endsWith("\\|")) row = row.slice(0, -1);
  return row.split(/(?<!\\)\|/).map((cell) => cell.trim());
}

function startsBlock(line: string, next: string | undefined): boolean {
  return (
    HEADING.test(line) ||
    FENCE.test(line) ||
    QUOTE.test(line) ||
    RULE.test(line) ||
    ITEM.test(line) ||
    (line.includes("|") && next !== undefined && TABLE_RULE.test(next))
  );
}

function indent(line: string): number {
  return line.length - line.trimStart().length;
}

export function parse(text: string, depth = 0): Block[] {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const at = (n: number) => lines[n] ?? "";
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = at(i);
    if (!line.trim()) {
      i++;
      continue;
    }
    const fence = FENCE.exec(line);
    if (fence) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !at(i).trimStart().startsWith(fence[1]!)) body.push(at(i++));
      i++;
      blocks.push({ kind: "code", text: body.join("\n") });
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      blocks.push({ kind: "heading", level: heading[1]!.length, inline: inline(heading[2]!) });
      i++;
      continue;
    }
    // Before lists: `- - -` and `* * *` are rules.
    if (RULE.test(line)) {
      blocks.push({ kind: "rule" });
      i++;
      continue;
    }
    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && QUOTE.test(at(i))) body.push(QUOTE.exec(at(i++))![1]!);
      blocks.push({ kind: "quote", children: depth < MAX_DEPTH ? parse(body.join("\n"), depth + 1) : [] });
      continue;
    }
    if (line.includes("|") && i + 1 < lines.length && TABLE_RULE.test(at(i + 1))) {
      const head = cells(line).map(inlineOf);
      const rows: Inline[][][] = [];
      i += 2;
      while (i < lines.length && at(i).includes("|") && at(i).trim()) {
        rows.push(cells(at(i)).map(inlineOf));
        i++;
      }
      blocks.push({ kind: "table", head, rows });
      continue;
    }
    if (ITEM.test(line)) {
      const [list, next] = parseList(lines, i, depth);
      blocks.push(list);
      i = next;
      continue;
    }
    const body = [line.trim()];
    i++;
    while (i < lines.length && at(i).trim() && !startsBlock(at(i), at(i + 1))) {
      body.push(at(i).trim());
      i++;
    }
    blocks.push({ kind: "paragraph", inline: inline(body.join("\n")) });
  }
  return blocks;
}

/// `inline` for `map`, which would pass the index as the depth.
function inlineOf(text: string): Inline[] {
  return inline(text);
}

function parseList(lines: string[], start: number, depth: number): [Block, number] {
  const at = (n: number) => lines[n] ?? "";
  const first = ITEM.exec(at(start))!;
  const base = first[1]!.length;
  const ordered = /\d/.test(first[2]!);
  const items: Item[] = [];
  let i = start;
  while (i < lines.length) {
    const match = ITEM.exec(at(i));
    if (!match || match[1]!.length !== base || /\d/.test(match[2]!) !== ordered) break;
    const own = [match[3]!];
    const nested: string[] = [];
    i++;
    while (i < lines.length) {
      const line = at(i);
      if (!line.trim()) {
        // A blank line ends the item unless more indented lines follow.
        if (i + 1 < lines.length && at(i + 1).trim() && indent(at(i + 1)) > base) {
          nested.push("");
          i++;
          continue;
        }
        break;
      }
      if (indent(line) > base) {
        if (nested.length === 0 && !ITEM.test(line) && !startsBlock(line, at(i + 1))) own.push(line.trim());
        else nested.push(line);
        i++;
        continue;
      }
      if (ITEM.test(line) || startsBlock(line, at(i + 1))) break;
      // A lazy continuation line.
      if (nested.length === 0) own.push(line.trim());
      else nested.push(line);
      i++;
    }
    const task = TASK.exec(own[0]!);
    if (task) own[0] = task[2]!;
    const minIndent = Math.min(...nested.filter((l) => l.trim()).map(indent));
    const children =
      nested.length && depth < MAX_DEPTH
        ? parse(nested.map((l) => l.slice(Math.min(minIndent, indent(l)))).join("\n"), depth + 1)
        : [];
    items.push({ inline: inline(own.join("\n")), checked: task ? task[1]! !== " " : null, children });
    // Items may be separated by one blank line.
    if (i < lines.length && !at(i).trim() && i + 1 < lines.length) {
      const after = ITEM.exec(at(i + 1));
      if (after && after[1]!.length === base) i++;
    }
  }
  return [{ kind: "list", ordered, items }, i];
}

const PUNCT = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

/// Text with times split out of it.
function texts(text: string, out: Inline[]): void {
  const re = /\[(\d{1,3}(?::\d{2}){1,2}(?:\s*[,，、]\s*\d{1,3}(?::\d{2}){1,2})*)\]/g;
  let last = 0;
  for (let match = re.exec(text); match; match = re.exec(text)) {
    const labels = match[1]!.split(/\s*[,，、]\s*/);
    const times = labels.map(timeMs);
    if (times.some((ms) => ms == null)) continue;
    if (match.index > last) push(out, text.slice(last, match.index));
    if (labels.length > 1) push(out, "[");
    labels.forEach((label, index) => {
      if (index > 0) push(out, ", ");
      out.push({ kind: "time", ms: times[index]!, label });
    });
    if (labels.length > 1) push(out, "]");
    last = match.index + match[0].length;
  }
  if (last < text.length) push(out, text.slice(last));
}

function push(out: Inline[], text: string): void {
  if (!text) return;
  const prev = out[out.length - 1];
  if (prev?.kind === "text") prev.text += text;
  else out.push({ kind: "text", text });
}

/// The closing delimiter for emphasis opened at `from`, or -1.
function closing(text: string, from: number, marker: string): number {
  for (let at = text.indexOf(marker, from); at >= 0; at = text.indexOf(marker, at + 1)) {
    if (text[at - 1] === "\\" || /\s/.test(text[at - 1] ?? " ")) continue;
    if (marker.length === 1 && text[at + 1] === marker) {
      at++;
      continue;
    }
    // `_` inside a word is not emphasis (snake_case).
    if (marker[0] === "_" && /\w/.test(text[at + marker.length] ?? "")) continue;
    return at;
  }
  return -1;
}

export function inline(text: string, depth = 0): Inline[] {
  const out: Inline[] = [];
  let plain = "";
  const flush = () => {
    texts(plain, out);
    plain = "";
  };
  let i = 0;
  while (i < text.length) {
    const ch = text[i]!;
    if (ch === "\\" && i + 1 < text.length && PUNCT.includes(text[i + 1]!)) {
      plain += text[i + 1];
      i += 2;
      continue;
    }
    if (ch === "`") {
      const run = /^`+/.exec(text.slice(i))![0];
      const end = text.indexOf(run, i + run.length);
      if (end > 0) {
        flush();
        out.push({ kind: "code", text: text.slice(i + run.length, end).trim() || text.slice(i + run.length, end) });
        i = end + run.length;
        continue;
      }
      plain += run;
      i += run.length;
      continue;
    }
    if ((ch === "*" || ch === "_") && depth < MAX_DEPTH) {
      const double = text[i + 1] === ch;
      const marker = double ? ch + ch : ch;
      const opensWord = !(ch === "_" && /\w/.test(text[i - 1] ?? ""));
      const next = text[i + marker.length];
      if (opensWord && next && !/\s/.test(next)) {
        const end = closing(text, i + marker.length + 1, marker);
        // Something besides markers inside: `****` is not emphasis.
        if (end > 0 && /[^*_\s]/.test(text.slice(i + marker.length, end))) {
          flush();
          const children = inline(text.slice(i + marker.length, end), depth + 1);
          out.push(double ? { kind: "strong", children } : { kind: "em", children });
          i = end + marker.length;
          continue;
        }
      }
      plain += marker;
      i += marker.length;
      continue;
    }
    // A link or image shows its text; the address is dropped.
    if (ch === "[" || (ch === "!" && text[i + 1] === "[")) {
      const open = ch === "!" ? i + 1 : i;
      const link = /^\[([^\]\n]*)\]\(([^)\s]*)(?:\s+"[^"]*")?\)/.exec(text.slice(open));
      if (link) {
        plain += link[1];
        i = open + link[0].length;
        continue;
      }
    }
    plain += ch;
    i++;
  }
  flush();
  return out;
}

/// The streamed notes without their `# title` line (the final notes keep it apart).
export function withoutTitle(markdown: string): string {
  const trimmed = markdown.trimStart();
  const end = trimmed.indexOf("\n");
  const first = end < 0 ? trimmed : trimmed.slice(0, end);
  if (/^#(\s|$)/.test(first.trim())) return end < 0 ? "" : trimmed.slice(end + 1);
  return markdown;
}

/// Plain text of a tree, for tests and accessible labels.
export function plainText(nodes: Inline[]): string {
  return nodes
    .map((node) => {
      switch (node.kind) {
        case "text":
        case "code":
          return node.text;
        case "time":
          return `[${node.label}]`;
        default:
          return plainText(node.children);
      }
    })
    .join("");
}
