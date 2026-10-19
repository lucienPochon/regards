import type { Editor } from "@tiptap/core";
import { type KeyboardEvent, Suspense, lazy, useCallback, useEffect, useRef, useState } from "react";
import {
  type Draft,
  type Queued,
  type SendCheck,
  attachDropped,
  checkDraft,
  describeCheck,
  attachFiles,
  deleteDraft,
  detachFile,
  onFilesDrag,
  onFilesDropped,
  formatRecipients,
  getDraft,
  invalidAddresses,
  parseRecipients,
  saveDraft,
  sendDraft,
  insertTemplateHtml,
  setDraftFrom,
  swapSignatureHtml,
  textToHtml,
} from "./compose";
import { type Identity, listIdentities } from "./identities";
import { IMAGE_TYPES, attachImage, draftImages, fileBase64, insertImage, toEditorHtml, toStoredHtml } from "./images";
import type { Template } from "./templates";
import { fileKind, fileSize, laterDate } from "./format";
import { Icon } from "./icons";
import { type Anchor, anchorOf } from "./Menu";
import { getPrefs } from "./prefs";
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
  pickTemplate,
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
  /**
   * Ask which template to start from (none: no templates button). What is
   * written so far comes along, so it can be kept as a template.
   */
  pickTemplate?: (anchor: Anchor, written: { subject: string; html: string }) => Promise<Template | null>;
}) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [fields, setFields] = useState<Fields | null>(null);
  const [copies, setCopies] = useState(false);
  const [status, setStatus] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  // What the daemon asked about before sending (send again to go anyway), and what was asked already.
  const [warnings, setWarnings] = useState<SendCheck[]>([]);
  const asked = useRef<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  // "Remind me if no reply by" this time, once it has gone.
  const [remindAt, setRemindAt] = useState<number | null>(null);
  // Who it can go out from, and a new editor when that changes the text.
  const [identities, setIdentities] = useState<Identity[]>([]);
  const [editorKey, setEditorKey] = useState(0);
  /** Files are dragged over the window: dropped, they are attached. */
  const [dragging, setDragging] = useState(false);
  const saved = useRef<string>("");
  /** The editor, to put images in. */
  const editor = useRef<Editor | null>(null);
  const keepEditor = useCallback((instance: Editor) => {
    editor.current = instance;
  }, []);
  const timer = useRef<number | undefined>(undefined);
  const toField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let current = true;
    getDraft(draftId)
      .then(async (loaded) => {
        // Images in the text are shown from their bytes while writing.
        let html = loaded.html ?? textToHtml(loaded.body);
        if (html.includes('src="cid:')) html = toEditorHtml(html, await draftImages(loaded.id));
        if (!current) return;
        const initial: Fields = {
          to: formatRecipients(loaded.to),
          cc: formatRecipients(loaded.cc),
          bcc: formatRecipients(loaded.bcc),
          subject: loaded.subject,
          html,
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
        html: toStoredHtml(fields.html),
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

  useShortcuts(
    {
      Escape: () => void close(),
      "Mod+Shift+T": () => {
        if (!locked) void insertTemplate({ x: 0, y: 0 });
      },
    },
    { modal: true },
  );

  const change = (name: keyof Fields) => (event: { target: { value: string } }) => {
    setError(null);
    setWarnings([]);
    setFields((current) => (current ? { ...current, [name]: event.target.value } : current));
  };

  /** Whether it can go as far as the fields show: someone to send to, and valid addresses. */
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
    return true;
  };

  /**
   * The daemon's checks on the saved draft (no subject, an attachment it
   * speaks of but lacks, something left to fill in): each is asked about
   * once, and a second press sends it anyway. True when nothing new is asked.
   */
  const confirmed = async (): Promise<boolean> => {
    if (!draft) return false;
    const key = (check: SendCheck) => (check.kind === "placeholder" ? `placeholder:${check.text}` : check.kind);
    const checks = await checkDraft(draft.id);
    if (checks.some((check) => check.kind === "no_recipients")) {
      setError(describeCheck({ kind: "no_recipients" }));
      return false;
    }
    const fresh = checks.filter((check) => !asked.current.has(key(check)));
    if (fresh.length === 0) return true;
    for (const check of fresh) asked.current.add(key(check));
    setWarnings(fresh);
    return false;
  };

  const send = async (sendAt: number | null = null) => {
    if (!draft || !checked()) return;
    setBusy(true);
    try {
      if (!(await flush()) || !(await confirmed())) return;
      onQueued(await sendDraft(draft.id, getPrefs().undoSeconds, sendAt, remindAt));
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
  };

  const later = async (button: HTMLElement) => {
    if (!checked()) return;
    try {
      if (!(await flush()) || !(await confirmed())) return;
    } catch (failure) {
      setError(String(failure));
      return;
    }
    const at = await pickSendTime(anchorOf(button, "end"));
    if (at !== null) await send(at);
  };

  /** A template's text where one writes, and its subject if there is none yet. */
  const insertTemplate = async (anchor: Anchor) => {
    if (!pickTemplate || !fields) return;
    const template = await pickTemplate(anchor, { subject: fields.subject, html: fields.html });
    if (!template) return;
    setFields((current) =>
      current
        ? {
            ...current,
            html: insertTemplateHtml(current.html, template),
            subject: current.subject.trim() ? current.subject : template.subject,
          }
        : current,
    );
    setEditorKey((key) => key + 1);
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

  // Files dropped on the window are attached to this draft.
  const openDraft = draft?.id;
  const frozen = draft?.sending ?? false;
  useEffect(() => {
    if (openDraft === undefined) return;
    let gone = false;
    const unlisten = [
      onFilesDrag((over) => !gone && setDragging(over)),
      onFilesDropped((dropped) => {
        if (gone) return;
        setDragging(false);
        if (frozen) {
          setError("This message is being sent. Undo it first to attach files.");
          return;
        }
        setError(null);
        // Images dropped on the text go in it; anything else beside it.
        const onText = document.elementFromPoint(dropped.x, dropped.y)?.closest(".compose-body") != null;
        const inline = onText && editor.current != null && dropped.files.every((file) => file.image);
        attachDropped(openDraft, dropped.id, inline)
          .then(async ({ draft: updated, refused }) => {
            if (updated) {
              setDraft(updated);
              if (inline && editor.current) {
                const shown = new Set(
                  [...editor.current.getHTML().matchAll(/data-cid="([^"]+)"/g)].map((found) => found[1]),
                );
                for (const image of await draftImages(openDraft)) {
                  if (!shown.has(image.content_id)) insertImage(editor.current, image);
                }
              }
            }
            if (refused.length) setError(refused.map((file) => `${file.name}: ${file.error}`).join("\n"));
          })
          .catch((failure) => setError(String(failure)));
      }),
    ];
    return () => {
      gone = true;
      for (const stop of unlisten) void stop.then((off) => off());
    };
  }, [openDraft, frozen]);

  /** Pasted images: stored, then put in the text. */
  const pasteImages = useCallback(
    async (files: File[]) => {
      if (!draft || draft.sending) return;
      setError(null);
      for (const file of files) {
        if (!IMAGE_TYPES.includes(file.type)) continue;
        try {
          const data = await fileBase64(file);
          const name = file.name || `image.${file.type.split("/")[1]}`;
          const stored = await attachImage(draft.id, { name, mime: file.type, data });
          setDraft(stored.draft);
          if (editor.current) insertImage(editor.current, { content_id: stored.content_id, mime: file.type, data });
        } catch (failure) {
          setError(String(failure));
        }
      }
    },
    [draft],
  );

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
    }
  };

  // Escape closes, even from the text: the editor would otherwise take it (to select the
  // enclosing block). The link field keeps it, to cancel the link.
  const escape = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || (event.target as HTMLElement).closest(".link-input")) return;
    event.preventDefault();
    event.stopPropagation();
    void close();
  };

  const locked = Boolean(draft?.sending) || busy;
  const hasRecipients = fields ? `${fields.to}${fields.cc}${fields.bcc}`.trim().length > 0 : false;
  // Images in the text are shown there, not as files.
  const files = draft?.attachments.filter((file) => !file.content_id) ?? [];
  const total = files.reduce((sum, file) => sum + file.size, 0);

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
      {pickTemplate && (
        <button
          type="button"
          className="icon-btn"
          title={`Insert a template (${MOD_KEY}+Shift+T)`}
          aria-label="Insert a template"
          onClick={(event) => void insertTemplate(anchorOf(event.currentTarget, "end"))}
          disabled={locked}
        >
          <Icon name="text" />
        </button>
      )}
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
    <section
      className={`composer ${mode}${dragging ? " dropping" : ""}`}
      role="dialog"
      aria-label={mode === "panel" ? "Reply" : "New message"}
      onKeyDown={keys}
      onKeyDownCapture={escape}
    >
      {dragging && (
        <div className="composer-drop" aria-hidden>
          Drop to attach
        </div>
      )}
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
                onImages={(pasted) => void pasteImages(pasted)}
                onEditor={keepEditor}
                disabled={locked}
                autofocus={!needsAddress}
              />
            </Suspense>
            {draft && files.length > 0 && (
              <div className="attachments">
                <p className="attachments-head">
                  <Icon name="paperclip" size={15} />
                  <strong>
                    {files.length} {files.length === 1 ? "attachment" : "attachments"}
                  </strong>
                  <span className="muted">{fileSize(total)}</span>
                </p>
                <ul className="tiles" aria-label="Attachments">
                  {files.map((file) => (
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
          {warnings.length > 0 && (
            <p className="composer-note" role="status">
              {warnings.map(describeCheck).join(" ")} Press Send again to send it anyway.
            </p>
          )}
          {error && <p className="composer-note error">{error}</p>}
          {mode === "panel" && tools}
        </>
      )}
    </section>
  );
}
