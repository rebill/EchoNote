import {
  formatExitCode,
  formatModelStatus,
  formatPid,
  formatServiceStatus
} from "../lib/formatting";
import type { SetupResponse } from "../lib/setup";

export function createRuntimeSummary(setup: SetupResponse): HTMLElement {
  const state = setup.state;
  const panel = createElement("section", "panel");
  panel.append(createElement("h2", undefined, "Runtime"));
  panel.append(
    createKeyValue("Service", formatServiceStatus(state.serviceStatus)),
    createKeyValue("Model", state.resolvedModelId),
    createKeyValue("Model status", formatModelStatus(state.modelStatus)),
    createKeyValue("API", state.baseUrl ?? "Unavailable"),
    createKeyValue("PID", formatPid(state.pid)),
    createKeyValue("Last exit", formatExitCode(state.lastExitCode))
  );

  if (state.lastError) {
    panel.append(createElement("p", "error-text", state.lastError));
  }

  return panel;
}

function createKeyValue(label: string, value: string): HTMLElement {
  const row = createElement("div", "key-value");
  row.append(createElement("span", undefined, label));
  row.append(createElement("strong", undefined, value));
  return row;
}

function createElement<K extends keyof HTMLElementTagNameMap>(
  tagName: K,
  className?: string,
  text?: string
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tagName);
  if (className) {
    element.className = className;
  }
  if (text !== undefined) {
    element.textContent = text;
  }
  return element;
}
