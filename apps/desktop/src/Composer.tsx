import type { Editor } from "@tiptap/core";
import { AddressInput } from "./AddressInput";
import { type KeyboardEvent, type MutableRefObject, Suspense, lazy, useCallback, useEffect, useRef, useState } from "react";
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
  isUntouched,
  parseRecipients,
  saveDraft,
  sendDraft,
  fillPlaceholders,
  insertTemplateHtml,
  setDraftFrom,
  swapSignatureBlocks,
  textSignature,
  textToHtml,
  touchDraft,
} from "./compose";
import { type Person, type UndoInfo, listAccounts } from "./accounts";
import { type Identity, listIdentities } from "./identities";
import { type Signature, editorSignature, listSignatures, signatureIn } from "./signatures";
import {
  IMAGE_TYPES,
  attachImage,
  attachPictures,
  type DraftImage,
  draftImages,
  fileBase64,
  insertImage,
  toEditorHtml,
  toStoredHtml,
} from "./images";
import { type Template, type Written, useTemplateOnDraft } from "./templates";
import { fileKind, fileSize, laterDate } from "./format";
import { Icon } from "./icons";
import { type Anchor, anchorOf, Menu, type MenuEntry, type MenuRequest } from "./Menu";
import { getPrefs } from "./prefs";
import { type CommandId, commandOf, useShortcuts, withKey } from "./shortcuts";
import { t } from "./i18n";

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
 * draft as you type; closing keeps the draft (one just started and left as
 * it came is deleted), the bin deletes it, Send queues it with a few seconds
 * to undo (with Ctrl+Shift+Enter, and files the conversation as Done), and
 * Send later picks a time. The bell asks to be reminded if nobody replies
 * by a time.
 */
export function Composer({
  draftId,
  mode,
  onMode,
  onClose,
  onDiscarded,
  onQueued,
  pickSendTime,
  pickReminder,
  pickTemplate,
  onAddAccount,
  onPopOut,
  saveRef,
}: {
  draftId: number;
  mode: "full" | "panel";
  /** Bigger or smaller; none in a window of its own. */
  onMode?: (mode: "full" | "panel") => void;
  /** Go on in a window of its own (saved first), with the subject for its title. */
  onPopOut?: (subject: string) => void;
  /** Given a way to save what is written now: a window of its own saves before it closes. */
  saveRef?: MutableRefObject<(() => Promise<boolean>) | null>;
  onClose: () => void;
  /** The bin deleted it: offer `undo`, which makes it again. */
  onDiscarded?: (undo: UndoInfo) => void;
  onQueued: (queued: Queued) => void;
  /** Ask when to send it: Unix ms, or null if cancelled. */
  pickSendTime: (anchor: Anchor) => Promise<number | null>;
  /** Ask when to be reminded if nobody replies: Unix ms, or null if cancelled. */
  pickReminder: (anchor: Anchor) => Promise<number | null>;
  /**
   * Ask which template to start from (none: no templates button). What is
   * written so far comes along, so it can be kept as a template.
   */
  pickTemplate?: (anchor: Anchor, written: Written) => Promise<Template | null>;
  /** From the From menu: go and add another account. */
  onAddAccount?: () => void;
}) {
  const [draft, setDraft] = useState<Draft | null>(null);
  /** The From menu, open. */
  const [fromMenu, setFromMenu] = useState<MenuRequest | null>(null);
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
  // The signatures to choose from (ADR-0021), and the one the text has.
  const [signatures, setSignatures] = useState<Signature[]>([]);
  const [signatureId, setSignatureId] = useState<number | null>(null);
  const signatureKnown = useRef(false);
  /** Files are dragged over the window: dropped, they are attached. */
  const [dragging, setDragging] = useState(false);
  const saved = useRef<string>("");
  /** The fields as the draft came: closed unchanged, a draft just started is not kept. */
  const initialFields = useRef<string>("");
  /** The editor, to put images in. */
  const editor = useRef<Editor | null>(null);
  const keepEditor = useCallback((instance: Editor) => {
    editor.current = instance;
  }, []);
  const timer = useRef<number | undefined>(undefined);
  const toField = useRef<HTMLInputElement>(null);
  const ccField = useRef<HTMLInputElement>(null);
  const bccField = useRef<HTMLInputElement>(null);
  const subjectField = useRef<HTMLInputElement>(null);
  const fromButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let current = true;
    signatureKnown.current = false;
    getDraft(draftId)
      .then(async (loaded) => {
        // Images in the text are shown from their bytes while writing, colours as the editor marks them.
        let html = loaded.html ?? textToHtml(loaded.body);
        html = toEditorHtml(html, html.includes('src="cid:') ? await draftImages(loaded.id) : []);
        if (!current) return;
        const initial: Fields = {
          to: formatRecipients(loaded.to),
          cc: formatRecipients(loaded.cc),
          bcc: formatRecipients(loaded.bcc),
          subject: loaded.subject,
          html,
        };
        saved.current = JSON.stringify(initial);
        initialFields.current = saved.current;
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

  // Every account's addresses, for the From menu: another account's moves the draft there.
  const [everyone, setEveryone] = useState<Identity[][]>([]);
  useEffect(() => {
    let current = true;
    listAccounts()
      .then((accounts) => Promise.all(accounts.map((account) => listIdentities(account.id))))
      .then(
        (lists) => current && setEveryone(lists.filter((list) => list.length > 0)),
        () => current && setEveryone([]),
      );
    return () => {
      current = false;
    };
  }, []);

  useEffect(() => {
    listSignatures().then(setSignatures, () => setSignatures([]));
  }, []);

  // The signature a draft has, recognised by its text once both are known. A
  // draft just started has a formatted signature's text only (the daemon puts
  // text in): the formatted one takes its place, its pictures attached.
  useEffect(() => {
    if (!draft || signatureKnown.current || signatures.length === 0) return;
    signatureKnown.current = true;
    const found = signatureIn(draft.body, signatures);
    setSignatureId(found?.id ?? null);
    if (!found?.html || !isUntouched(draft.id)) return;
    attachPictures(draft.id, editorSignature(found)).then(
      (formatted) => {
        setFields((now) => {
          if (!now) return now;
          const next = { ...now, html: swapSignatureBlocks(now.html, textSignature(found.body), formatted) };
          // Still as it opened: closed as it is, it is not kept.
          if (initialFields.current === JSON.stringify(now)) initialFields.current = JSON.stringify(next);
          return next;
        });
        setEditorKey((key) => key + 1);
      },
      (failure: unknown) => setError(String(failure)),
    );
  }, [draft, signatures]);

  /** The signature the message has now, as the editor has it. */
  const currentSignature = () => {
    const chosen = signatures.find((signature) => signature.id === signatureId);
    if (chosen) return editorSignature(chosen);
    return textSignature(identities.find((identity) => identity.id === draft?.identity_id)?.signature ?? "");
  };

  /** Another signature (or none, `null`) as the editor puts it in: formatted ones with their pictures attached. */
  const signatureToPut = async (draftId: number, signature: Signature | null, text = "") =>
    signature ? attachPictures(draftId, editorSignature(signature)) : textSignature(text);

  /** Send it from another identity: its signature takes the old one's place. */
  const changeFrom = async (identityId: number) => {
    if (!draft || !fields) return;
    // Of this account, or another, where the draft then goes.
    const next = [...identities, ...everyone.flat()].find((identity) => identity.id === identityId);
    if (!next) return;
    touchDraft(draft.id);
    setError(null);
    try {
      const old = currentSignature();
      const moved = await setDraftFrom(draft.id, next.id);
      const chosen = signatures.find((signature) => signature.id === next.signature_id) ?? null;
      const put = await signatureToPut(moved.id, chosen, next.signature);
      setDraft(moved);
      setFields({ ...fields, html: swapSignatureBlocks(fields.html, old, put) });
      setSignatureId(next.signature_id);
      setEditorKey((key) => key + 1);
    } catch (failure) {
      setError(String(failure));
    }
  };

  /**
   * The From menu: every account's addresses, account by account, the one it
   * goes from ticked; then adding an account.
   */
  const openFrom = (button: HTMLElement) => {
    if (!draft) return;
    const current = draft.identity_id ?? identities.find((identity) => identity.is_default)?.id;
    const accounts = everyone.length ? everyone : [identities];
    const addresses: MenuEntry[] = accounts.some((list) => list.length > 0)
      ? accounts.flatMap((list, at) => [
          ...(at > 0 ? ["separator" as const] : []),
          ...list.map((identity) => ({
            label: identity.email,
            detail: identity.name || undefined,
            icon: identity.id === current ? ("check" as const) : ("user" as const),
            onSelect: () => void changeFrom(identity.id),
          })),
        ])
      : [{ label: draft.from.address ?? "", detail: draft.from.name ?? undefined, icon: "check", disabled: true, onSelect: () => undefined }];
    setFromMenu({
      anchor: anchorOf(button, "end"),
      label: t("Send from"),
      entries: onAddAccount
        ? [...addresses, "separator", { label: t("Add an account…"), icon: "plus", onSelect: onAddAccount }]
        : addresses,
    });
  };

  /** Another signature, or none: it takes the old one's place in the text. */
  const changeSignature = async (value: string) => {
    if (!fields || !draft) return;
    const next = signatures.find((signature) => String(signature.id) === value) ?? null;
    try {
      const put = await signatureToPut(draft.id, next);
      setFields({ ...fields, html: swapSignatureBlocks(fields.html, currentSignature(), put) });
      setSignatureId(next?.id ?? null);
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
    // Started here and closed as it came (Esc after Reply): nothing to keep,
    // nor to put in the server's Drafts.
    if (draft && isUntouched(draft.id) && fields && JSON.stringify(fields) === initialFields.current) {
      window.clearTimeout(timer.current);
      touchDraft(draft.id);
      await deleteDraft(draft.id).catch(() => undefined);
      onClose();
      return;
    }
    await flush();
    onClose();
  }, [draft, fields, flush, onClose]);
  useEffect(() => {
    if (saveRef) saveRef.current = flush;
  }, [flush, saveRef]);

  useShortcuts({ Escape: () => void close() }, { modal: true });

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
      setError(t("Add at least one recipient."));
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

  /** Send now or at `sendAt`; with `done`, the conversation it answers is filed as Done too. */
  const send = async (sendAt: number | null = null, done = false) => {
    if (!draft || !checked()) return;
    setBusy(true);
    try {
      if (!(await flush()) || !(await confirmed())) return;
      const queued = await sendDraft(draft.id, getPrefs().undoSeconds, sendAt, remindAt);
      onQueued(done ? { ...queued, done } : queued);
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

  /**
   * A template's text where one writes, and its subject if there is none
   * yet, its name placeholders filled from the first recipient and the
   * sender (add the recipient first, as in Spark).
   */
  const insertTemplate = async (anchor: Anchor) => {
    if (!pickTemplate || !fields || !draft) return;
    // Kept as a message is (colours as styles, images by Content-ID), with its recipients and files.
    const template = await pickTemplate(anchor, {
      subject: fields.subject,
      html: toStoredHtml(fields.html),
      to: parseRecipients(fields.to),
      cc: parseRecipients(fields.cc),
      bcc: parseRecipients(fields.bcc),
      draftId: draft.id,
    });
    if (!template) return;
    touchDraft(draft.id);
    // Its files (and the images in its text) on the draft first, so the text can show them.
    let images: DraftImage[] = [];
    if (template.files.length > 0) {
      try {
        setDraft(await useTemplateOnDraft(draft.id, template.id));
        if (template.files.some((file) => file.inline)) images = await draftImages(draft.id);
      } catch (failure) {
        setError(String(failure));
      }
    }
    // Its recipients join the message's, each once.
    const join = (now: string, more: Person[]) => {
      const have = parseRecipients(now);
      const known = new Set(have.map((person) => person.address?.toLowerCase()));
      return formatRecipients([...have, ...more.filter((person) => !known.has(person.address?.toLowerCase()))]);
    };
    const to = join(fields.to, template.to);
    const first = parseRecipients(to)[0];
    const names = { to: first?.name ?? null, me: draft.from.name ?? null };
    const filled = {
      body: fillPlaceholders(template.body, names),
      html: template.html === null ? null : toEditorHtml(fillPlaceholders(template.html, names, true), images),
    };
    if (template.cc.length + template.bcc.length > 0) setCopies(true);
    setFields((current) =>
      current
        ? {
            ...current,
            to: join(current.to, template.to),
            cc: join(current.cc, template.cc),
            bcc: join(current.bcc, template.bcc),
            html: insertTemplateHtml(current.html, filled),
            subject: current.subject.trim() ? current.subject : fillPlaceholders(template.subject, names),
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
      touchDraft(draft.id);
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
          setError(t("This message is being sent. Undo it first to attach files."));
          return;
        }
        setError(null);
        // Images dropped on the text go in it; anything else beside it.
        const onText = document.elementFromPoint(dropped.x, dropped.y)?.closest(".compose-body") != null;
        const inline = onText && editor.current != null && dropped.files.every((file) => file.image);
        attachDropped(openDraft, dropped.id, inline)
          .then(async ({ draft: updated, refused }) => {
            if (updated) {
              touchDraft(openDraft);
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
          touchDraft(draft.id);
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
      touchDraft(draft.id);
      setDraft(await detachFile(draft.id, attachmentId));
    } catch (failure) {
      setError(String(failure));
    }
  };

  const discard = async () => {
    if (!draft) return;
    window.clearTimeout(timer.current);
    try {
      const { undo } = await deleteDraft(draft.id);
      if (undo) onDiscarded?.(undo);
      onClose();
    } catch (failure) {
      setError(String(failure));
    }
  };

  /** Cc and Bcc, shown first if they were not. */
  const copyField = (field: MutableRefObject<HTMLInputElement | null>) => {
    if (copies) return field.current?.focus();
    setCopies(true);
    window.requestAnimationFrame(() => field.current?.focus());
  };

  // The composer's keys, from any field (Settings › Shortcuts › Writing): with Ctrl
  // (Cmd on a Mac), unless changed, Enter sends, Shift+Enter sends and files the
  // conversation as Done, Shift+A attaches, Shift+T inserts a template, Shift+\
  // deletes the draft, and 1–6 go to From, Subject, To, Cc, Bcc and the text.
  const keys = (event: KeyboardEvent) => {
    // The editor sends from its text itself (RichEditor), before its own keys.
    if (event.defaultPrevented) return;
    const run: Partial<Record<CommandId, () => void>> = {
      send: () => void send(null, false),
      sendDone: () => void send(null, true),
      attach: () => void attach(),
      template: () => {
        if (!locked) void insertTemplate({ x: 0, y: 0 });
      },
      discard: () => void discard(),
      toFrom: () => fromButton.current?.focus(),
      toSubject: () => subjectField.current?.focus(),
      toTo: () => toField.current?.focus(),
      toCc: () => copyField(ccField),
      toBcc: () => copyField(bccField),
      toText: () => editor.current?.commands.focus(),
    };
    const command = commandOf(event.nativeEvent, Object.keys(run) as CommandId[]);
    if (command === null) return;
    event.preventDefault();
    run[command]?.();
  };

  // Escape closes, even from the text: the editor would otherwise take it (to select the
  // enclosing block). The link field keeps it, to cancel the link.
  const escape = (event: KeyboardEvent) => {
    // A link being typed, an open list of people, or the From menu takes Escape itself.
    if (event.key !== "Escape" || (event.target as HTMLElement).closest(".link-input, .address-input.suggesting, .menu-layer"))
      return;
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
      <button type="button" className="icon-btn" title={withKey(t("Attach files"), "attach")} aria-label={t("Attach files")} onClick={() => void attach()} disabled={locked}>
        <Icon name="paperclip" />
      </button>
      <button
        type="button"
        className="icon-btn danger"
        title={withKey(t("Discard the draft"), "discard")}
        aria-label={t("Discard the draft")}
        onClick={() => void discard()}
        disabled={Boolean(draft?.sending)}
      >
        <Icon name="trash" />
      </button>
      {pickTemplate && (
        <button
          type="button"
          className="icon-btn"
          title={withKey(t("Insert a template"), "template")}
          aria-label={t("Insert a template")}
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
            ? t("Remind me if nobody replies")
            : t("Reminds you {when} if nobody replies (click to remove)", { when: laterDate(remindAt) })
        }
        aria-label={t("Remind me if nobody replies")}
        aria-pressed={remindAt !== null}
        onClick={(event) => void remind(event.currentTarget)}
        disabled={locked}
      >
        <Icon name="bell" />
      </button>
      <button
        type="button"
        className="icon-btn"
        title={t("Send later")}
        aria-label={t("Send later")}
        onClick={(event) => void later(event.currentTarget)}
        disabled={locked || !hasRecipients}
      >
        <Icon name="sendLater" />
      </button>
      <span className="sep" />
      {signatures.length > 0 && (
        <select
          className="composer-from"
          aria-label={t("Signature")}
          title={t("Signature")}
          value={signatureId ?? ""}
          onChange={(event) => void changeSignature(event.target.value)}
          disabled={locked}
        >
          <option value="">{t("No signature")}</option>
          {signatures.map((signature) => (
            <option key={signature.id} value={signature.id}>
              {signature.name}
            </option>
          ))}
        </select>
      )}
      {draft && (
        <button
          ref={fromButton}
          type="button"
          className="composer-from-button"
          aria-haspopup="menu"
          aria-label={`From ${formatRecipients([draft.from])}`}
          title={t("Send from")}
          onClick={(event) => openFrom(event.currentTarget)}
          disabled={locked}
        >
          <span className="composer-from-text">
            <span>{draft.from.address}</span>
            {draft.from.name && <small>{draft.from.name}</small>}
          </span>
          <Icon name="upDown" size={14} />
        </button>
      )}
      <button
        type="button"
        className="send-button"
        title={withKey(t("Send"), "send")}
        aria-label={t("Send")}
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
      aria-label={mode === "panel" ? t("Reply") : t("New message")}
      onKeyDown={keys}
      onKeyDownCapture={escape}
    >
      {dragging && (
        <div className="composer-drop" aria-hidden>
          {t("Drop to attach")}
        </div>
      )}
      {/* The title, then the window's buttons in the top right corner, Close last. */}
      <div className="composer-top">
        <span className="composer-title">{mode === "panel" ? fields?.subject || t("New message") : ""}</span>
        <div className="composer-window">
          {onMode && (
            <button
              type="button"
              className="icon-btn"
              title={mode === "panel" ? t("Full size") : t("Smaller")}
              aria-label={mode === "panel" ? t("Full size") : t("Smaller")}
              onClick={() => onMode(mode === "panel" ? "full" : "panel")}
            >
              <Icon name={mode === "panel" ? "expand" : "shrink"} size={15} />
            </button>
          )}
          {onPopOut && (
            <button
              type="button"
              className="icon-btn"
              title={t("Open in a new window")}
              aria-label={t("Open in a new window")}
              onClick={() => void flush().then(() => onPopOut(fields?.subject ?? ""))}
            >
              <Icon name="popOut" size={15} />
            </button>
          )}
          <button
            type="button"
            className="icon-btn"
            title={t("Close (Esc). The draft is kept, once something is written in it.")}
            aria-label={t("Close")}
            onClick={() => void close()}
          >
            <Icon name="close" size={16} />
          </button>
        </div>
      </div>
      {!fields ? (
        <p className={error ? "composer-note error" : "composer-note muted"} role={error ? "alert" : "status"}>
          {error ?? t("Opening…")}
        </p>
      ) : (
        <>
          {draft?.sending && (
            <p className="composer-note error" role="status">
              {t("This message is being sent. Undo it first to change it.")}
            </p>
          )}
          <fieldset className="composer-fields" disabled={locked}>
            <input
              ref={subjectField}
              className="subject-field"
              value={fields.subject}
              onChange={change("subject")}
              placeholder={t("Subject")}
              aria-label={t("Subject")}
            />
            {/* Not a label: a label may hold one control, and this line also has the Cc Bcc button. */}
            <div className="address-line">
              <span onClick={() => toField.current?.focus()}>{t("To")}</span>
              <AddressInput
                inputRef={toField}
                label={t("To")}
                value={fields.to}
                onChange={(to) => change("to")({ target: { value: to } })}
                accountId={accountId ?? null}
                besides={`${fields.cc}, ${fields.bcc}`}
                placeholder={t("name@example.org, …")}
              />
              {!copies && (
                <button type="button" className="link" onClick={() => setCopies(true)}>
                  {t("Cc Bcc")}
                </button>
              )}
            </div>
            {copies && (
              <>
                {/* Not labels either: the chips are controls too. */}
                <div className="address-line">
                  <span onClick={() => ccField.current?.focus()}>{t("Cc")}</span>
                  <AddressInput
                    inputRef={ccField}
                    label={t("Cc")}
                    value={fields.cc}
                    onChange={(cc) => change("cc")({ target: { value: cc } })}
                    accountId={accountId ?? null}
                    besides={`${fields.to}, ${fields.bcc}`}
                  />
                </div>
                <div className="address-line">
                  <span onClick={() => bccField.current?.focus()}>{t("Bcc")}</span>
                  <AddressInput
                    inputRef={bccField}
                    label={t("Bcc")}
                    value={fields.bcc}
                    onChange={(bcc) => change("bcc")({ target: { value: bcc } })}
                    accountId={accountId ?? null}
                    besides={`${fields.to}, ${fields.cc}`}
                  />
                </div>
              </>
            )}
            <Suspense fallback={<p className="muted small">{t("Opening the editor…")}</p>}>
              <RichEditor
                key={editorKey}
                initial={fields.html}
                onChange={(html) => {
                  setError(null);
                  setFields((current) => (current ? { ...current, html } : current));
                }}
                onSend={(done) => void send(null, done)}
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
                <ul className="tiles" aria-label={t("Attachments")}>
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
                        title={t("Remove")}
                        onClick={() => void detach(file.id)}
                      >
                        <Icon name="close" size={12} />
                      </button>
                    </li>
                  ))}
                  <li className="tile add">
                    <button type="button" className="tile-face" aria-label={t("Attach more files")} onClick={() => void attach()}>
                      <Icon name="plus" size={22} />
                    </button>
                  </li>
                </ul>
              </div>
            )}
          </fieldset>
          {warnings.length > 0 && (
            <p className="composer-note" role="status">
              {warnings.map(describeCheck).join(" ")} {t("Press Send again to send it anyway.")}
            </p>
          )}
          {error && (
            <p className="composer-note error" role="alert">
              {error}
            </p>
          )}
          {tools}
        </>
      )}
      {fromMenu && <Menu request={fromMenu} onClose={() => setFromMenu(null)} />}
    </section>
  );
}
