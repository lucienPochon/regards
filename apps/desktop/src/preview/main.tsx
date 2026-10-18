// The UI with sample mail, in a browser: `npm run dev`, then open
// http://localhost:1420/preview.html. Add `?accounts=2` for two accounts,
// `?accounts=0` for the first-run screen, and `?theme=dark` or `?theme=light`.
import "./mock";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../App";
import { type Theme, applyTheme, savedTheme } from "../theme";
import "../styles.css";

const params = new URLSearchParams(window.location.search);
const theme = params.get("theme");
applyTheme(theme === "dark" || theme === "light" ? (theme as Theme) : savedTheme());
// `?layout=split` and `?sidebar=open` start from that layout; without them, from the default.
if (params.has("layout") || params.has("sidebar")) {
  window.localStorage.setItem(
    "regards.layout",
    JSON.stringify({ mode: params.get("layout") === "split" ? "split" : "single", locked: params.get("sidebar") === "open" }),
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

// `?do=click:.row-main|key:j|right:.row-main` plays steps after loading, for screenshots
// (click or right-click the first element matching a selector, or press a key).
const steps = new URLSearchParams(window.location.search).get("do")?.split("|") ?? [];
void (async () => {
  for (const step of steps) {
    await new Promise((resolve) => window.setTimeout(resolve, 400));
    const [kind, ...rest] = step.split(":");
    const value = rest.join(":");
    const element = kind === "key" ? null : document.querySelector<HTMLElement>(value);
    if (kind === "click") element?.click();
    else if (kind === "right" && element) {
      const box = element.getBoundingClientRect();
      element.dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: box.left + 60, clientY: box.top + 20 }),
      );
    } else if (kind === "key") {
      const shift = value.startsWith("Shift+");
      const key = shift ? value.slice(6) : value;
      window.dispatchEvent(new KeyboardEvent("keydown", { key, shiftKey: shift, bubbles: true }));
    }
  }
})();
