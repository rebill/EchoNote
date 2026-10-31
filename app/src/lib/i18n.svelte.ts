// The UI language and `t()`. Components that call `t` re-render when the language changes.

import { en, zh, type StringKey } from "./strings";
import type { CommandError, Language } from "./types";

export const locale = $state<{ value: Language }>({ value: "zh-CN" });

export function translate(
  language: Language,
  key: StringKey,
  params: Record<string, string | number> = {},
): string {
  const template = language === "en" ? en[key] : zh[key];
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

export function t(key: StringKey, params?: Record<string, string | number>): string {
  return translate(locale.value, key, params);
}

/// A command error in the UI language.
export function errorText(error: CommandError): string {
  const key = `error.${error.code}` as StringKey;
  return key in zh ? t(key) : t("error.generic", { message: error.message });
}
