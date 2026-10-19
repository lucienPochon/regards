import { invoke } from "@tauri-apps/api/core";
import type { Draft } from "./compose";

/**
 * The assistant (ADR-0015 §5): with the person's own key, a conversation
 * summarised or a reply drafted, only when they ask. Off until a provider
 * is chosen in Settings. What a model writes is text to read and check: a
 * summary is shown, a reply lands in a draft and is never sent by itself.
 */

/** Anthropic's API, or any server with OpenAI's chat completions API. */
export type AssistantKind = "anthropic" | "openai_compatible";

/** Mirrors `AssistantSettings`. The key never comes back. */
export interface AssistantSettings {
  /** `null`: off (the default). */
  kind: AssistantKind | null;
  base_url: string | null;
  model: string | null;
  /** A key is kept in the system's credential store. */
  has_key: boolean;
}

/** Mirrors `AssistantSetSettingsParams`. */
export interface AssistantChoice {
  /** `null` turns the assistant off and forgets the key. */
  kind: AssistantKind | null;
  /**
   * Anthropic's own when omitted. For an OpenAI-compatible server, its base,
   * up to `/v1`: `https://…/v1`, or `http://localhost:11434/v1` on this
   * computer (`http://` is for this computer only).
   */
  base_url?: string;
  /** `claude-opus-5-5` for Anthropic when omitted; needed otherwise. */
  model?: string;
  /**
   * A new key. Omitted: the one kept stays, unless the server changed (a
   * key goes only where it was given for). A server on this computer may need none.
   */
  api_key?: string;
}

/** Mirrors `AssistantTask`. */
export type AssistantTask = { kind: "summary" } | { kind: "reply"; instructions: string };

/** Mirrors `AssistantPreview`: what would go, before it goes. */
export interface AssistantPreview {
  /** Messages of the conversation that would be sent: the latest. */
  messages: number;
  /** Characters of their text, together (without what they quote). */
  characters: number;
  /** Left out: they look like they carry a sign-in or verification code. */
  left_out: number;
  /** The host that would get them. */
  destination: string;
  model: string;
}

/** Mirrors `AssistantResult`. */
export interface AssistantResult {
  /** The summary, or the reply's text: written by a model, to check. */
  text: string;
  /** For a reply: the draft it was written into, to review and send (or not). */
  draft: Draft | null;
}

export const assistantSettings = () => invoke<AssistantSettings>("assistant_settings");

export const setAssistantSettings = (settings: AssistantChoice) =>
  invoke<AssistantSettings>("set_assistant_settings", { settings });

/** What `runAssistant` would send for this message's conversation, and where. */
export const previewAssistant = (messageId: number, task: AssistantTask) =>
  invoke<AssistantPreview>("assistant_preview", { messageId, task });

/** Can take a while (up to two minutes): show that it is working. */
export const runAssistant = (messageId: number, task: AssistantTask, all = false) =>
  invoke<AssistantResult>("assistant_run", { messageId, task, all });

/** "2 messages (1,234 characters) go to api.anthropic.com, for claude-opus-5-5. 1 with a code is left out." */
export function describePreview(preview: AssistantPreview): string {
  const count = (n: number, one: string, many: string) => `${n.toLocaleString("en")} ${n === 1 ? one : many}`;
  const sent =
    `${count(preview.messages, "message", "messages")} (${count(preview.characters, "character", "characters")}) ` +
    `${preview.messages === 1 ? "goes" : "go"} to ${preview.destination}, for ${preview.model}.`;
  if (preview.left_out === 0) return sent;
  return `${sent} ${count(preview.left_out, "message", "messages")} with a code ${preview.left_out === 1 ? "is" : "are"} left out.`;
}
