# ADR-0021: Several signatures, one default per address

- **Status:** Accepted (2026-10-22, after a reviewed prototype)
- **Date:** 2026-10-22
- **Deciders:** Project owner

## Context

Today each identity (each address an account sends as) has one plain-text signature (migration 0016):

- **Where it goes.** The daemon adds it when it creates a draft, below what is written and above a quote (`compose::signature_block`).
- **Changing address.** Sending a draft from another address swaps the old address's signature for the new one's. The daemon swaps it in plain-text drafts (`compose::swap_signature`), the composer in formatted ones (`swapSignatureHtml`).
- **Editing.** It is edited in Settings › Accounts › Addresses & signatures, next to the name and address.

People keep several signatures: a formal one, a short one for replies, one in English, one for a team address. The owner uses a mail client that offers exactly that:

- a list of signatures;
- one default per account;
- a way to switch signature from inside a message.

The owner asked for the same, and approved a prototype of it (alongside ADR-0020).

## Decision

1. **Signatures are a list of their own**, kept in a `signatures` table:
   - each has a name (for the person only) and a plain-text body, at most 5,000 characters;
   - they belong to the person, not to an account, so one signature can serve several addresses.
2. **Each identity has one default signature, or none**, kept as `identities.signature_id`.
   - It goes into new mail, replies and forwards from that address.
   - Deleting a signature leaves the addresses that used it with none.
3. **The migration keeps what people have.** Each identity's non-empty signature becomes a signature named after the address, and that identity's default. Identical texts become one signature. The old `identities.signature` column goes.
4. **Drafts work as today**, with the default in place of the old field:
   - the daemon adds the identity's default signature when it creates a draft;
   - changing address swaps the old address's default for the new one's, with the same swap rules.
5. **Switching inside a message.** The composer shows which signature the message has, and lets the person:
   - step to the previous or next one;
   - pick one from a list;
   - choose *No signature*;
   - open the signature settings.

   The swap uses the same rules as changing address: the old text is replaced where it is, or the new one goes above the quote. When a saved draft is opened again, its signature is recognised by its text.
6. **Plain text, for now.** Formatting (bold, links, a logo) needs an HTML body and HTML-aware swapping. It is left for later, and the editor's toolbar comes with it.
7. **Protocol:**
   - `signature.list`, `signature.save` and `signature.remove` are `ui` only;
   - `identity.save` takes `signature_id` in place of the signature text;
   - identity results keep a read-only `signature` field: the default's text, so the composer and the daemon swap as before.

   Agents don't manage signatures. Their drafts get the default, as the person's do.
8. **Where it shows:**
   - **Settings › Composer › Signatures:**
     - *Default signatures*, with one choice per address;
     - *All signatures*, with one row per signature, each opening its editor (name, text, *Delete*, *Save*);
     - *New signature* at the top right.
   - **Settings › Accounts › Addresses** keeps the name and address, without the signature text.

## Alternatives considered

- **Keep one signature per address.** Nothing to build, but no formal and short versions, and a signature written again for each alias. Rejected.
- **Signatures kept per account.** Each account's list would stay separate, but the same signature would have to be written again for each account. Rejected: one list, with defaults per address.
- **Separate defaults for new mail and for replies.** It was in the first prototype. One default plus switching in the message proved simpler. Rejected for now.
- **Formatted signatures now.** Wanted eventually, but they change how signatures are swapped in drafts. Left for a later step (§6).

## Consequences

- **Store:** a `signatures` table, `identities.signature_id`, and the migration of existing signatures.
- **Daemon:**
  - the signature methods;
  - `identity.save` and identity results change;
  - draft creation and `draft.set_from` read the default.
- **App:**
  - the Signatures page;
  - the address editor loses its signature box;
  - the composer gets the signature switcher.
- **Preview mock:** the mock follows the new methods.
- **Small PRs, in order:** store and daemon; TS API and composer; settings screens.

## References

- [ADR-0020](0020-voice-and-notes.md) (voice and notes, the same prototype)
- Migration `0016_identities.sql`; `compose::signature_block` and `compose::swap_signature`
