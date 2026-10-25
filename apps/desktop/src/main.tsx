import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { composerWindowDraft } from "./compose";
import { ComposerWindow } from "./ComposerWindow";
import { applyTheme, savedTheme } from "./theme";
import "./styles.css";
import { spoken } from "./i18n";

applyTheme(savedTheme());

// Screen readers and spelling follow the app's language.
document.documentElement.lang = spoken();
const root = document.getElementById("root");
if (!root) {
  throw new Error("missing #root element");
}

// A message in a window of its own (its label is compose-<draft id>): the composer alone.
const draftId = composerWindowDraft();
createRoot(root).render(<StrictMode>{draftId === null ? <App /> : <ComposerWindow draftId={draftId} />}</StrictMode>);
