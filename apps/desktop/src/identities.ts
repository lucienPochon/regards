import { invoke } from "@tauri-apps/api/core";

/** One of the names and addresses an account sends as, with its signature. */
export interface Identity {
  id: number;
  account_id: string;
  name: string;
  email: string;
  /** Plain text, put below what is written and above a quote. */
  signature: string;
  /** How agents should write as this address (ADR-0020); kept while off. */
  voice: string;
  /** Whether agents are given the voice. */
  voice_on: boolean;
  is_default: boolean;
}

/** Most characters a voice may have. */
export const VOICE_MAX = 2000;

/**
 * Set an address's voice, or switch it on or off (the text is kept). Only
 * the app can: an agent's suggestion arrives as a `voice` hold instead.
 */
export const setVoice = (id: number, voice: string, on: boolean) =>
  invoke<Identity>("identity_set_voice", { id, voice, on });

/** The one-line request to hand an agent so it learns a voice from the person's sent mail (ADR-0020 §3). */
export const learnVoicePrompt = (email: string) =>
  `Learn how I write from my sent mail at ${email} and suggest it as my voice in Regards.`;

/** An account's identities, the default first. */
export const listIdentities = (accountId: string) =>
  invoke<{ identities: Identity[] }>("identity_list", { accountId }).then((result) => result.identities);

/** Add one (no `id`) or change one. */
export const saveIdentity = (identity: {
  id: number | null;
  account_id: string;
  name: string;
  email: string;
  signature: string;
}) => invoke<Identity>("identity_save", { identity: { ...identity, id: identity.id ?? undefined } });

/** Not the default: make another one the default first. */
export const removeIdentity = (id: number) => invoke("identity_remove", { id });
export const makeDefaultIdentity = (id: number) => invoke("identity_make_default", { id });
