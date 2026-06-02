const SUMMARY_SECTION_HEADINGS = ["Summary", "Decisions", "Action Items", "Key Points", "Open Questions"] as const;

export type SummarySectionHeading = (typeof SUMMARY_SECTION_HEADINGS)[number];

export type SummarySectionContent = Record<SummarySectionHeading, string>;

type MarkdownSection = {
  heading: string;
  startIndex: number;
  contentStartIndex: number;
  endIndex: number;
};

export function extractTranscript(markdown: string): string {
  const transcriptSection = findSections(markdown).find((section) => section.heading === "Transcript");
  if (!transcriptSection) {
    return "";
  }

  return markdown.slice(transcriptSection.contentStartIndex, transcriptSection.endIndex).trim();
}

export function replaceSummarySections(markdown: string, content: SummarySectionContent): string {
  let nextMarkdown = markdown;
  for (const heading of SUMMARY_SECTION_HEADINGS) {
    nextMarkdown = replaceOrInsertSection(nextMarkdown, heading, content[heading]);
  }
  return nextMarkdown;
}

export function replaceTranscriptSection(markdown: string, content: string): string {
  const normalizedContent = `\n\n${content.trim()}\n`;
  const sections = findSections(markdown);
  const existing = sections.find((section) => section.heading === "Transcript");

  if (existing) {
    return `${markdown.slice(0, existing.contentStartIndex)}${normalizedContent}${markdown.slice(existing.endIndex)}`;
  }

  return `${markdown.trimEnd()}\n\n## Transcript${normalizedContent}`;
}

export function replaceMeetingEndTime(markdown: string, endTime: string): string {
  const normalizedEndTime = endTime.trim();
  if (!normalizedEndTime) {
    return markdown;
  }

  return markdown.replace(/^(-[ \t]*Time:[ \t]*[^\n]*?[ \t]+-[ \t]*)[^\n]*$/m, `$1${normalizedEndTime}`);
}

function replaceOrInsertSection(markdown: string, heading: SummarySectionHeading, content: string): string {
  const sections = findSections(markdown);
  const existing = sections.find((section) => section.heading === heading);
  const normalizedContent = `\n\n${content.trim() || "_None._"}\n\n`;

  if (existing) {
    return `${markdown.slice(0, existing.contentStartIndex)}${normalizedContent}${markdown.slice(existing.endIndex)}`;
  }

  const transcript = sections.find((section) => section.heading === "Transcript");
  const insertAt = transcript?.startIndex ?? markdown.length;
  const sectionMarkdown = `## ${heading}${normalizedContent}`;
  return `${markdown.slice(0, insertAt)}${sectionMarkdown}\n${markdown.slice(insertAt)}`;
}

function findSections(markdown: string): MarkdownSection[] {
  const headingRegex = /^##\s+(.+?)\s*$/gm;
  const matches = [...markdown.matchAll(headingRegex)];

  return matches.map((match, index) => {
    const startIndex = match.index ?? 0;
    const contentStartIndex = startIndex + match[0].length;
    const nextMatch = matches[index + 1];
    const endIndex = nextMatch?.index ?? markdown.length;
    return {
      heading: match[1].trim(),
      startIndex,
      contentStartIndex,
      endIndex
    };
  });
}
