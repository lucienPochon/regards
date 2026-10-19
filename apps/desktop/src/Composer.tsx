import { type KeyboardEvent, Suspense, lazy, useCallback, useEffect, useRef, useState } from "react";
import {
  type Draft,
  type Queued,
  attachFiles,
  deleteDraft,
  detachFile,
  formatRecipients,
  getDraft,
  invalidAddresses,
  parseRecipients,
  saveDraft,
  sendDraft,
  setDraftFrom,
  swapSignatureHtml,
  textToHtml,
} from "./compose";
import { type Identity, listIdentities } from "./identities";
import { fileKind, fileSize, laterDate } from "./format";
import { Icon } from "./icons";
import { type Anchor, anchorOf } from "./Menu";
import { MOD_KEY, useShortcuts } from "./shortcuts";

// The editor (Tiptap and ProseMirror) loads when a message is first written.
const RichEditor = lazy(() => import("./RichEditor").then((module) => ({ default: module.RichEditor })));

interface Fields {
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  /** The formatted text, as the editor has it. */
  html: string;
}

/** How long typing pauses before the draft is saved. */
const AUTOSAVE_MS = 800;

/**
 * Write a message: across the whole content area ("full"), or in a panel
 * over the conversation when replying ("panel"). Everything is saved to the
 * draft as you type; closing keeps the draft, the bin deletes it, Send queues
 * it with a few seconds to undo, and Send later picks a time. The bell asks
 * to be reminded if nobody replies by a time.
 */
export function Composer({
  draftId,
  mode,
  onMode,
  onClose,
  onQueued,
  pickSendTime,
  pickReminder,
}: {
  draftId: number;
  mode: "full" | "panel";
  onMode: (mode: "full" | "panel") => void;
  onClose: () => void;
  onQueued: (queued: Queued) => void;
  /** Ask when to send it: Unix ms, or null if cancelled. */
  pickSendTime: (anchor: Anchor) => Promise<number | null>;
  /** Ask when to be reminded if nobody replies: Unix ms, or null if cancelled. */
  pickReminder: (anchor: Anchor) => Promise<number | null>;
}) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [fields, setFields] = useState<Fields | null>(null);
  const [copies, setCopies] = useState(false);
  const [status, setStatus] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const [askSubject, setAskSubject] = useState(false);
  const [busy, setBusy] = useState(false);
  // "Remind me if no reply by" this time, once it has gone.
  const [remindAt, setRemindAt] = useState<number | null>(null);
  // Who it can go out from, and a new editor when that changes the text.
  const [identities, setIdentities] = useState<Identity[]>([]);
  const [editorKey, setEditorKey] = useState(0);
  const saved = useRef<string>("");
  const timer = useRef<number | undefined>(undefined);
  const toField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let current = true;
    getDraft(draftId)
      .then((loaded) => {
        if (!current) return;
        const initial: Fields = {
          to: formatRecipients(loaded.to),
          cc: formatRecipients(loaded.cc),
          bcc: formatRecipients(loaded.bcc),
          subject: loaded.subject,
          html: loaded.html ?? textToHtml(loaded.body),
        };
        saved.current = JSON.stringify(initial);
        setDraft(loaded);
        setFields(initial);
        setCopies(loaded.cc.length + loaded.bcc.length > 0);
      })
      .catch((failure: unknown) => current && setError(String(failure)));
    return () => {
      current = false;
    };
  }, [draftId]);

  const accountId = draft?.account_id;
  useEffect(() => {
    if (accountId === undefined) return;
    listIdentities(accountId).then(setIdentities, () => setIdentities([]));
  }, [accountId]);

  /** Send it from another identity: its signature takes the old one's place. */
  const changeFrom = async (identityId: number) => {
    if (!draft || !fields) return;
    const old = identities.find((identity) => identity.id === draft.identity_id);
    const next = identities.find((identity) => identity.id === identityId);
    if (!next) return;
    setError(null);
    try {
      setDraft(await setDraftFrom(draft.id, next.id));
      setFields({ ...fields, html: swapSignatureHtml(fields.html, old?.signature ?? "", next.signature) });
      setEditorKey((key) => key + 1);
    } catch (failure) {
      setError(String(failure));
    }
  };

  // Start where the writing goes: the address for a new message (the editor
  // takes the focus itself for a reply). Once per draft, not on every change.
  const draftKey = draft?.id;
  const needsAddress = draft ? draft.to.length === 0 : false;
  useEffect(() => {
    if (draftKey !== undefined && needsAddress) toField.current?.focus();
  }, [draftKey]);

  /** Save now if anything changed since the last save. */
  const flush = useCallback(async (): Promise<boolean> => {
    window.clearTimeout(timer.current);
    if (!fields || !draft) return false;
    const snapshot = JSON.stringify(fields);
    if (snapshot === saved.current) return true;
    setStatus("Saving…");
    try {
      await saveDraft({
        id: draft.id,
        to: parseRecipients(fields.to),
        cc: parseRecipients(fields.cc),
        bcc: parseRecipients(fields.bcc),
        subject: fields.subject,
        body: "",
        html: fields.html,
      });
      saved.current = snapshot;
      setStatus("Saved");
      return true;
    } catch (failure) {
      setStatus("");
      setError(String(failure));
      return false;
    }
  }, [draft, fields]);

  useEffect(() => {
    if (!fields) return;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void flush(), AUTOSAVE_MS);
    return () => window.clearTimeout(timer.current);
  }, [fields, flush]);

  const close = useCallback(async () => {
    await flush();
    onClose();
  }, [flush, onClose]);

  useShortcuts({ Escape: () => void close() }, { modal: true });

  const change = (name: keyof Fields) => (event: { target: { value: string } }) => {
    setError(null);
    setAskSubject(false);
    setFields((current) => (current ? { ...current, [name]: event.target.value } : current));
  };

  /** Whether it can go: someone to send to, valid addresses, and a subject (or a second press). */
  const checked = (): boolean => {
    if (!fields || !draft || busy) return false;
    const everyone = [...parseRecipients(fields.to), ...parseRecipients(fields.cc), ...parseRecipients(fields.bcc)];
    if (everyone.length === 0) {
      setError("Add at least one recipient.");
      return false;
    }
    const invalid = invalidAddresses(everyone);
    if (invalid.length > 0) {
      setError(`Check ${invalid.length === 1 ? "this address" : "these addresses"}: ${invalid.join(", ")}`);
      return false;
    }
    if (!fields.subject.trim() && !askSubject) {
      setAskSubject(true);
      return false;
    }
    return true;
  };

  const send = async (sendAt: number | null = null) => {
    if (!draft || !checked()) return;
    setBusy(true);
    try {
      if (!(await flush())) return;
      onQueued(await sendDraft(draft.id, 10, sendAt, remindAt));
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
  };

  const later = async (button: HTMLElement) => {
    if (!checked()) return;
    const at = await pickSendTime(anchorOf(button, "end"));
    if (at !== null) await send(at);
  };

  const remind = async (button: HTMLElement) => {
    if (remindAt !== null) {
      setRemindAt(null);
      return;
    }
    setRemindAt(await pickReminder(anchorOf(button, "end")));
  };

  const attach = async () => {
    if (!draft) return;
    setError(null);
    try {
      const updated = await attachFiles(draft.id);
      if (updated) setDraft(updated);
    } catch (failure) {
      setError(String(failure));
    }
  };

  const detach = async (attachmentId: number) => {
    if (!draft) return;
    try {
      setDraft(await detachFile(draft.id, attachmentId));
    } catch (failure) {
      setError(String(failure));
    }
  };

  const discard = async () => {
    if (!draft) return;
    window.clearTimeout(timer.current);
    try {
      await deleteDraft(draft.id);
      onClose();
    } catch (failure) {
      setError(String(failure));
    }
  };

  const keys = (event: KeyboardEvent) => {
    // The editor handles its own keys (Cmd/Ctrl+Enter sends from there too).
    if (event.defaultPrevented) return;
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      void send();
    } else if (event.key === "Escape") {
      event.preventDefault();
      void close();
    }
  };

  const locked = Boolean(draft?.sending) || busy;
  const hasRecipients = fields ? `${fields.to}${fields.cc}${fields.bcc}`.trim().length > 0 : false;
  const total = draft?.attachments.reduce((sum, file) => sum + file.size, 0) ?? 0;

  const tools = (
    <div className="composer-tools">
      <span className="composer-status">{status}</span>
      <button type="button" className="icon-btn" title="Attach files" aria-label="Attach files" onClick={() => void attach()} disabled={locked}>
        <Icon name="paperclip" />
      </button>
      <button
        type="button"
        className="icon-btn danger"
        title="Discard the draft"
        aria-label="Discard the draft"
        onClick={() => void discard()}
        disabled={Boolean(draft?.sending)}
      >
        <Icon name="trash" />
      </button>
      <button
        type="button"
        className="icon-btn"
        title={
          remindAt === null
            ? "Remind me if nobody replies"
            : `Reminds you ${laterDate(remindAt)} if nobody replies (click to remove)`
        }
        aria-label="Remind me if nobody replies"
        aria-pressed={remindAt !== null}
        onClick={(event) => void remind(event.currentTarget)}
        disabled={locked}
      >
        <Icon name="bell" />
      </button>
      <button
        type="button"
        className="icon-btn"
        title="Send later"
        aria-label="Send later"
        onClick={(event) => void later(event.currentTarget)}
        disabled={locked || !hasRecipients}
      >
        <Icon name="sendLater" />
      </button>
      <span className="sep" />
      {draft && identities.length > 1 ? (
        <select
          className="composer-from"
          aria-label="From"
          title={formatRecipients([draft.from])}
          value={draft.identity_id ?? ""}
          onChange={(event) => void changeFrom(Number(event.target.value))}
          disabled={locked}
        >
          {identities.map((identity) => (
            <option key={identity.id} value={identity.id}>
              {identity.name ? `${identity.name} <${identity.email}>` : identity.email}
            </option>
          ))}
        </select>
      ) : (
        draft && (
          <span className="composer-from" title={formatRecipients([draft.from])}>
            {draft.from.address}
          </span>
        )
      )}
      <button
        type="button"
        className="send-button"
        title={`Send (${MOD_KEY}+Enter)`}
        aria-label="Send"
        onClick={() => void send()}
        disabled={locked || !hasRecipients}
      >
        <Icon name="send" size={20} />
      </button>
    </div>
  );

  return (
    <section className={`composer ${mode}`} role="dialog" aria-label={mode === "panel" ? "Reply" : "New message"} onKeyDown={keys}>
      <div className="composer-top">
        <div className="segmented">
          <button type="button" title="Close (Esc). The draft is kept." aria-label="Close" onClick={() => void close()}>
            <Icon name="close" size={16} />
          </button>
          <button
            type="button"
            title={mode === "panel" ? "Bigger" : "Smaller"}
            aria-label={mode === "panel" ? "Bigger" : "Smaller"}
            onClick={() => onMode(mode === "panel" ? "full" : "panel")}
          >
            <Icon name={mode === "panel" ? "expand" : "shrink"} size={15} />
          </button>
        </div>
        {mode === "panel" ? <span className="composer-title">{fields?.subject || "New message"}</span> : tools}
      </div>
      {!fields ? (
        <p className={error ? "composer-note error" : "composer-note muted"}>{error ?? "Opening…"}</p>
      ) : (
        <>
          {draft?.sending && <p className="composer-note error">This message is being sent. Undo it first to change it.</p>}
          <fieldset className="composer-fields" disabled={locked}>
            <input
              className="subject-field"
              value={fields.subject}
              onChange={change("subject")}
              placeholder="Subject"
              aria-label="Subject"
            />
            <label className="address-line">
              <span>To</span>
              <input ref={toField} value={fields.to} onChange={change("to")} placeholder="name@example.org, …" />
              {!copies && (
                <button type="button" className="link" onClick={() => setCopies(true)}>
                  Cc Bcc
                </button>
              )}
            </label>
            {copies && (
              <>
                <label className="address-line">
                  <span>Cc</span>
                  <input value={fields.cc} onChange={change("cc")} />
                </label>
                <label className="address-line">
                  <span>Bcc</span>
                  <input value={fields.bcc} onChange={change("bcc")} />
                </label>
              </>
            )}
            <Suspense fallback={<p className="muted small">Opening the editor…</p>}>
              <RichEditor
                key={editorKey}
                initial={fields.html}
                onChange={(html) => {
                  setError(null);
                  setFields((current) => (current ? { ...current, html } : current));
                }}
                onSend={() => void send()}
                disabled={locked}
                autofocus={!needsAddress}
              />
            </Suspense>
            {draft && draft.attachments.length > 0 && (
              <div className="attachments">
                <p className="attachments-head">
                  <Icon name="paperclip" size={15} />
                  <strong>
                    {draft.attachments.length} {draft.attachments.length === 1 ? "attachment" : "attachments"}
                  </strong>
                  <span className="muted">{fileSize(total)}</span>
                </p>
                <ul className="tiles" aria-label="Attachments">
                  {draft.attachments.map((file) => (
                    <li key={file.id} className="tile">
                      <span className="tile-face">{fileKind(file.name, file.mime)}</span>
                      <span className="tile-name" title={file.name}>
                        {file.name}
                      </span>
                      <span className="tile-size">{fileSize(file.size)}</span>
                      <button
                        type="button"
                        className="tile-remove"
                        aria-label={`Remove ${file.name}`}
                        title="Remove"
                        onClick={() => void detach(file.id)}
                      >
                        <Icon name="close" size={12} />
                      </button>
                    </li>
                  ))}
                  <li className="tile add">
                    <button type="button" className="tile-face" aria-label="Attach more files" onClick={() => void attach()}>
                      <Icon name="plus" size={22} />
                    </button>
                  </li>
                </ul>
              </div>
            )}
          </fieldset>
          {askSubject && <p className="composer-note">There is no subject. Press Send again to send it anyway.</p>}
          {error && <p className="composer-note error">{error}</p>}
          {mode === "panel" && tools}
        </>
      )}
    </section>
  );
}
