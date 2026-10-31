// Times and dates for display. Pure functions: the language is passed in.

import type { Language } from "./types";

/// `m:ss`, or `h:mm:ss` from one hour.
export function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const ss = String(seconds).padStart(2, "0");
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, "0")}:${ss}` : `${minutes}:${ss}`;
}

/// "1 小时 5 分钟" / "1 h 5 min"; under a minute "< 1 分钟" / "< 1 min".
export function duration(ms: number, language: Language): string {
  const minutes = Math.floor(ms / 60_000);
  const zh = language !== "en";
  if (minutes < 1) return zh ? "< 1 分钟" : "< 1 min";
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) return zh ? `${rest} 分钟` : `${rest} min`;
  if (rest === 0) return zh ? `${hours} 小时` : `${hours} h`;
  return zh ? `${hours} 小时 ${rest} 分钟` : `${hours} h ${rest} min`;
}

/// "512 B", "3.4 KB", "10.2 MB", "1.5 GB".
export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = n / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

/// Minutes east of UTC at `ms` (UTC+8 is 480), for exports in local time.
export function utcOffsetMinutes(ms: number): number {
  return -new Date(ms).getTimezoneOffset();
}

/// Date and time of day in the local time zone.
export function dateTime(ms: number, language: Language): string {
  return new Intl.DateTimeFormat(language, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(ms));
}

/// Short form for lists: time today, else month and day (with the year when not this one).
export function shortDate(ms: number, language: Language, now = Date.now()): string {
  const date = new Date(ms);
  const today = new Date(now);
  if (date.toDateString() === today.toDateString()) {
    return new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit" }).format(date);
  }
  const options: Intl.DateTimeFormatOptions =
    date.getFullYear() === today.getFullYear()
      ? { month: "short", day: "numeric" }
      : { year: "numeric", month: "short", day: "numeric" };
  return new Intl.DateTimeFormat(language, options).format(date);
}
