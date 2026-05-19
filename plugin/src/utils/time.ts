export function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

export function formatClockTime(date: Date): string {
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}`;
}

export function formatDate(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

export function formatMeetingTitle(format: string, date: Date): string {
  return format
    .replaceAll("YYYY", String(date.getFullYear()))
    .replaceAll("MM", pad2(date.getMonth() + 1))
    .replaceAll("DD", pad2(date.getDate()))
    .replaceAll("HH", pad2(date.getHours()))
    .replaceAll("mm", pad2(date.getMinutes()))
    .replaceAll("ss", pad2(date.getSeconds()));
}

export function sanitizeFileName(fileName: string): string {
  return fileName.replace(/[\\/:*?"<>|]/g, "-").trim();
}

export function formatTranscriptTimestamp(offsetMs: number): string {
  const totalSeconds = Math.max(0, Math.floor(offsetMs / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) {
    return `${pad2(hours)}:${pad2(minutes)}:${pad2(seconds)}`;
  }

  return `${pad2(minutes)}:${pad2(seconds)}`;
}
