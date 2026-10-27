import { invoke } from "@tauri-apps/api/core";
import { textSignature } from "./compose";
import type { Identity } from "./identities";
import { stylesForEditor } from "./images";

/**
 * Signatures (ADR-0021): a list for every account, each named for the
 * person; each address uses one as its default, or none. Plain text, or
 * formatted with pictures (`html`), its text then kept beside it.
 */
export interface Signature {
  id: number;
  /** For the person only: "Formal", "Short". */
  name: string;
  /** Its text; with `html`, the formatted version's. */
  body: string;
  /** The formatted version, its pictures embedded (`data:`), colours as styles. */
  html?: string | null;
  updated_at: number;
}

/** A signature as the composer's editor holds it: its formatted version (styles marked), or its text's; nothing for none. */
export const editorSignature = (signature: Signature | null): string =>
  signature?.html ? stylesForEditor(signature.html) : textSignature(signature?.body ?? "");

/** Most characters a signature may have. */
export const SIGNATURE_MAX = 5000;

/** Every signature, by name. */
export const listSignatures = () =>
  invoke<{ signatures: Signature[] }>("signature_list").then((result) => result.signatures);

/** Add one (no `id`) or change one. */
export const saveSignature = (signature: { id: number | null; name: string; body: string; html?: string | null }) =>
  invoke<Signature>("signature_save", { signature: { ...signature, id: signature.id ?? undefined } });

/** Delete one: the addresses that used it have none from then on. */
export const removeSignature = (id: number) => invoke("signature_remove", { id });

/** An address's default signature, or none (`null`). */
export const setDefaultSignature = (identityId: number, signatureId: number | null) =>
  invoke<Identity>("identity_set_signature", { id: identityId, signatureId });

/** The signature a message's text has, among `signatures`: the one whose text it holds, the longest first. */
export function signatureIn(text: string, signatures: Signature[]): Signature | null {
  const found = signatures
    .filter((signature) => signature.body.trim() && text.includes(signature.body.trimEnd()))
    .sort((a, b) => b.body.length - a.body.length);
  return found[0] ?? null;
}
