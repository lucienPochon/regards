import { invoke } from "@tauri-apps/api/core";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { composerWindowDraft } from "./compose";
import { ComposerWindow } from "./ComposerWindow";
import { ThreadWindow } from "./ThreadWindow";
import { threadWindowMessage } from "./thread-window";
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
// A conversation in a window of its own (thread-<message id>): the reader alone.
const threadId = threadWindowMessage();
// The shell (tray, notifications, dialogs) speaks the window's language too.
if (draftId === null && threadId === null) invoke("set_language", { language: spoken() }).catch(() => {});
createRoot(root).render(<StrictMode>{draftId !== null ? <ComposerWindow draftId={draftId} /> : threadId !== null ? <ThreadWindow messageId={threadId} /> : <App />}</StrictMode>);
