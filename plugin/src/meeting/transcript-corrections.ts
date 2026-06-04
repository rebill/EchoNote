export type TranscriptCorrectionRule = {
  from: string;
  to: string;
};

export function parseTranscriptCorrectionRules(rulesText: string): TranscriptCorrectionRule[] {
  const rules = new Map<string, string>();

  for (const rawLine of rulesText.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) {
      continue;
    }

    const separatorIndex = line.indexOf("=>");
    if (separatorIndex < 0) {
      continue;
    }

    const from = line.slice(0, separatorIndex).trim();
    const to = line.slice(separatorIndex + 2).trim();
    if (!from || !to) {
      continue;
    }

    rules.set(from, to);
  }

  return Array.from(rules.entries())
    .map(([from, to]) => ({ from, to }))
    .sort((left, right) => right.from.length - left.from.length);
}

export function applyTranscriptCorrections(text: string, rulesText: string): string {
  if (!text || !rulesText.trim()) {
    return text;
  }

  let corrected = text;
  for (const rule of parseTranscriptCorrectionRules(rulesText)) {
    corrected = corrected.split(rule.from).join(rule.to);
  }
  return corrected;
}
