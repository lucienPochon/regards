// The UI with sample mail, in a browser: `npm run dev`, then open
// http://localhost:1420/preview.html. The Tour button (bottom right) lists
// every screen and state; or add `?accounts=2` for two accounts,
// `?accounts=0` for the first-run screen, and `?theme=dark` or `?theme=light`.
import "./mock";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../App";
import { type Theme, applyTheme, savedTheme } from "../theme";
import { Tour } from "./Tour";
import "../styles.css";

const params = new URLSearchParams(window.location.search);
const theme = params.get("theme");
applyTheme(theme === "dark" || theme === "light" ? (theme as Theme) : savedTheme());
// `?layout=single`, `?sidebar=icons` and `?view=smart|cards|date` start from that layout; without
// them, from the default (split view, sidebar open, the inbox by date).
if (params.has("layout") || params.has("sidebar") || params.has("view")) {
  window.localStorage.setItem(
    "regards.layout",
    JSON.stringify({
      mode: params.get("layout") === "single" ? "single" : "split",
      locked: params.get("sidebar") !== "icons",
      inboxView: params.get("view") ?? undefined,
    }),
  );
} else if (params.has("fresh")) {
  window.localStorage.removeItem("regards.layout");
}

const root = document.getElementById("root");
if (!root) throw new Error("missing #root element");
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

// The Tour panel, in its own root beside the app.
const tour = document.createElement("div");
document.body.append(tour);
createRoot(tour).render(<Tour />);

// `?do=click:.row-main|key:j|right:.row-main` plays steps after loading, for screenshots and
// the Tour: wait:ms, click:css, right:css (right-click), text:css=words (click the first match
// containing the words), see:css=words (scroll it into view), key:Mod+Shift+k (Mod is Ctrl, or Cmd on a Mac; sent to the focused
// element), type:words (into the focused field), settings:Page (open Settings at that page),
// call:drop (files dropped on the window), call:link=hold:2 (a regards:// link).
const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
const sleep = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));
const hooks = window as unknown as { regardsDropFiles: (holdMs?: number) => void; regardsOpenLink: (link: string) => void };

function byText(selector: string, words: string) {
  return [...document.querySelectorAll<HTMLElement>(selector)].find((element) => element.textContent?.includes(words));
}

function press(value: string) {
  const parts = value.length > 1 ? value.split("+") : [value];
  const key = parts.pop() ?? value;
  const mod = parts.includes("Mod");
  const target = document.activeElement instanceof HTMLElement ? document.activeElement : document.body;
  target.dispatchEvent(
    new KeyboardEvent("keydown", {
      key,
      shiftKey: parts.includes("Shift"),
      ctrlKey: mod && !isMac,
      metaKey: mod && isMac,
      bubbles: true,
      cancelable: true,
    }),
  );
}

async function play(step: string) {
  const [kind, ...rest] = step.split(":");
  const value = rest.join(":");
  if (kind === "wait") return sleep(Number(value));
  if (kind === "click") document.querySelector<HTMLElement>(value)?.click();
  else if (kind === "text") {
    const at = value.indexOf("=");
    byText(value.slice(0, at), value.slice(at + 1))?.click();
  } else if (kind === "see") {
    const at = value.indexOf("=");
    byText(value.slice(0, at), value.slice(at + 1))?.scrollIntoView({ block: "center" });
  } else if (kind === "right") {
    const element = document.querySelector<HTMLElement>(value);
    if (!element) return;
    const box = element.getBoundingClientRect();
    element.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: box.left + 60, clientY: box.top + 20 }),
    );
  } else if (kind === "key") press(value);
  else if (kind === "type") {
    const field = document.activeElement;
    if (field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) {
      const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(field), "value")?.set;
      setter?.call(field, value);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    }
  } else if (kind === "settings") {
    press("Mod+,");
    await sleep(350);
    if (value === "Accounts") document.querySelector<HTMLElement>(".settings-me")?.click();
    else byText(".settings-item", value)?.click();
  } else if (kind === "call") {
    if (value === "drop") hooks.regardsDropFiles(4000);
    else if (value.startsWith("link=")) hooks.regardsOpenLink(value.slice(5));
  }
}

const steps = new URLSearchParams(window.location.search).get("do")?.split("|") ?? [];
void (async () => {
  for (const step of steps) {
    await sleep(400);
    await play(step);
  }
})();
