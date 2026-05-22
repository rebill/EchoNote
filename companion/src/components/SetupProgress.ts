import type { SetupStep, SetupStepStatus } from "../lib/setup";

export function createSetupProgress(steps: SetupStep[]): HTMLElement {
  const panel = createElement("section", "panel setup-progress-panel");
  panel.append(createElement("h2", undefined, "Setup"));

  const list = createElement("ol", "setup-steps");
  for (const step of steps) {
    const item = createElement("li", `setup-step setup-step-${step.status}`);
    const marker = createElement("span", "setup-step-marker", markerFor(step.status));
    const body = createElement("div", "setup-step-body");
    body.append(createElement("strong", undefined, step.label));
    body.append(createElement("span", undefined, step.summary));
    if (step.detail && (step.status === "failed" || step.status === "warning")) {
      body.append(createElement("code", undefined, step.detail));
    }
    item.append(marker, body);
    list.append(item);
  }

  panel.append(list);
  return panel;
}

function markerFor(status: SetupStepStatus): string {
  switch (status) {
    case "passed":
      return "✓";
    case "running":
      return "...";
    case "failed":
    case "warning":
      return "!";
    case "skipped":
      return "-";
    case "pending":
      return "○";
  }
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
