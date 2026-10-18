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
  textToHtml,
} from "./compose";
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
 * Write a message. Everything is saved to the draft as you type; closing keeps
 * the draft, Discard deletes it, Send queues it with a few seconds to undo.
 */
export function Composer({
  draftId,
  onClose,
  onQueued,
}: {
  draftId: number;
  onClose: () => void;
  onQueued: (queued: Queued) => void;
}) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [fields, setFields] = useState<Fields | null>(null);
  const [copies, setCopies] = useState(false);
  const [status, setStatus] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const [askSubject, setAskSubject] = useState(false);
  const [busy, setBusy] = useState(false);
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

  const send = async () => {
    if (!fields || !draft || busy) return;
    const everyone = [...parseRecipients(fields.to), ...parseRecipients(fields.cc), ...parseRecipients(fields.bcc)];
    if (everyone.length === 0) {
      setError("Add at least one recipient.");
      return;
    }
    const invalid = invalidAddresses(everyone);
    if (invalid.length > 0) {
      setError(`Check ${invalid.length === 1 ? "this address" : "these addresses"}: ${invalid.join(", ")}`);
      return;
    }
    if (!fields.subject.trim() && !askSubject) {
      setAskSubject(true);
      return;
    }
    setBusy(true);
    try {
      if (!(await flush())) return;
      onQueued(await sendDraft(draft.id));
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
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

  return (
    <div className="help-backdrop">
      <section className="card composer" role="dialog" aria-modal="true" aria-label="New message" onKeyDown={keys}>
        {!fields ? (
          <p className={error ? "error" : "muted"}>{error ?? "Opening…"}</p>
        ) : (
          <>
            {draft?.sending && <p className="error small">This message is being sent. Undo it first to change it.</p>}
            <fieldset disabled={draft?.sending || busy}>
              {draft && (
                <p className="field from">
                  <span>From</span>
                  <span>{formatRecipients([draft.from])}</span>
                </p>
              )}
              <label className="field">
                <span>To</span>
                <input ref={toField} value={fields.to} onChange={change("to")} placeholder="name@example.org, …" />
                {!copies && (
                  <button type="button" className="link" onClick={() => setCopies(true)}>
                    Cc/Bcc
                  </button>
                )}
              </label>
              {copies && (
                <>
                  <label className="field">
                    <span>Cc</span>
                    <input value={fields.cc} onChange={change("cc")} />
                  </label>
                  <label className="field">
                    <span>Bcc</span>
                    <input value={fields.bcc} onChange={change("bcc")} />
                  </label>
                </>
              )}
              <label className="field">
                <span>Subject</span>
                <input value={fields.subject} onChange={change("subject")} />
              </label>
              <Suspense fallback={<p className="muted small">Opening the editor…</p>}>
              <RichEditor
                initial={fields.html}
                onChange={(html) => {
                  setError(null);
                  setFields((current) => (current ? { ...current, html } : current));
                }}
                onSend={() => void send()}
                disabled={Boolean(draft?.sending) || busy}
                autofocus={!needsAddress}
              />
              </Suspense>
              {draft && draft.attachments.length > 0 && (
                <ul className="attachment-chips" aria-label="Attachments">
                  {draft.attachments.map((file) => (
                    <li key={file.id}>
                      <span>{file.name}</span>
                      <span className="muted"> · {fileSize(file.size)}</span>
                      <button
                        type="button"
                        className="link"
                        aria-label={`Remove ${file.name}`}
                        onClick={() => void detach(file.id)}
                      >
                        ×
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </fieldset>
            {askSubject && (
              <p className="small">
                There is no subject. Press Send again to send it anyway.
              </p>
            )}
            {error && <p className="error small">{error}</p>}
            <div className="actions-bar">
              <button type="button" onClick={() => void send()} disabled={draft?.sending || busy}>
                Send
              </button>
              <button type="button" onClick={() => void attach()} disabled={draft?.sending || busy}>
                Attach files
              </button>
              <button type="button" onClick={() => void close()}>
                Close
              </button>
              <button type="button" className="danger" onClick={() => void discard()} disabled={draft?.sending}>
                Discard
              </button>
              <span className="muted small">{status || `${MOD_KEY}+Enter sends`}</span>
            </div>
          </>
        )}
      </section>
    </div>
  );
}

function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
