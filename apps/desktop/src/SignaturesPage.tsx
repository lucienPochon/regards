import type { Editor } from "@tiptap/core";
import { Suspense, lazy, useCallback, useEffect, useRef, useState } from "react";
import type { AccountSummary } from "./accounts";
import type { ConfirmRequest } from "./Confirm";
import { textSignature } from "./compose";
import { type Identity, listIdentities } from "./identities";
import { Icon } from "./icons";
import { coloursAsMarks, coloursAsStyles, fileBase64 } from "./images";
import { LinkRow, Option, Select, SubPageHead } from "./SettingsControls";
import { type Signature, SIGNATURE_MAX, listSignatures, removeSignature, saveSignature, setDefaultSignature } from "./signatures";
import { t, tn } from "./i18n";

const RichEditor = lazy(() => import("./RichEditor").then((module) => ({ default: module.RichEditor })));

/** A signature's picture, at most: the daemon keeps it inside the signature (base64, 256 KB). */
const PICTURE_MAX = 190 * 1024;

/** "No signature" in an address's choice: signature ids start at 1. */
const NONE = 0;

/**
 * Settings › Composer › Signatures (ADR-0021): the signature each address
 * adds by default, and every signature, each opening its editor. One
 * signature can serve several addresses; a message can switch to another
 * in the composer.
 */
export function SignaturesPage({
  accounts,
  confirm,
  onBack,
}: {
  accounts: AccountSummary[];
  confirm: (request: ConfirmRequest) => Promise<boolean>;
  onBack: () => void;
}) {
  const [signatures, setSignatures] = useState<Signature[] | null>(null);
  const [identities, setIdentities] = useState<Identity[]>([]);
  const [editing, setEditing] = useState<Signature | "new" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const accountIds = accounts.map((account) => account.id).join(",");
  const reload = useCallback(() => {
    const failed = (failure: unknown) => setError(String(failure));
    listSignatures().then(setSignatures, failed);
    Promise.all(accountIds.split(",").filter(Boolean).map(listIdentities)).then((lists) => setIdentities(lists.flat()), failed);
  }, [accountIds]);
  useEffect(reload, [reload]);

  if (editing) {
    const signature = editing === "new" ? null : editing;
    return (
      <SignatureEditor
        signature={signature}
        usedBy={signature ? identities.filter((identity) => identity.signature_id === signature.id).map((identity) => identity.email) : []}
        confirm={confirm}
        onBack={() => {
          setEditing(null);
          reload();
        }}
      />
    );
  }

  const choose = (identity: Identity, id: number) => {
    setError(null);
    setDefaultSignature(identity.id, id === NONE ? null : id).then(reload, (failure: unknown) => setError(String(failure)));
  };
  const options = [{ id: NONE, label: t("No signature") }, ...(signatures ?? []).map((signature) => ({ id: signature.id, label: signature.name }))];

  return (
    <>
      <div className="signatures-head">
        <SubPageHead title={t("Signatures")} onBack={onBack} />
        <button type="button" className="btn small" onClick={() => setEditing("new")}>
          <Icon name="plus" size={15} />
          {t("New signature")}
        </button>
      </div>
      <p className="muted settings-lead">
        {t("Added below what you write and above a quote, with its formatting and pictures. In a message, the signature menu next to the sender switches it.")}
      </p>
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      <h4 className="settings-subhead">{t("Default signatures")}</h4>
      {identities.map((identity) => (
        <Option key={identity.id} icon="user" label={identity.email} description={identity.name || undefined}>
          <Select
            label={t("Default signature for {address}", { address: identity.email })}
            value={identity.signature_id ?? NONE}
            options={options}
            onChange={(id) => choose(identity, id)}
          />
        </Option>
      ))}
      <h4 className="settings-subhead">{t("All signatures")}</h4>
      {signatures?.length === 0 && (
        <p className="muted settings-note">{t("None yet. A formal one and a short one for replies are a good start.")}</p>
      )}
      {signatures?.map((signature) => (
        <LinkRow
          key={signature.id}
          icon="text"
          label={signature.name}
          detail={signature.body.trim().split("\n")[0]}
          onOpen={() => setEditing(signature)}
        />
      ))}
    </>
  );
}

/** One signature's name and text, saved or deleted; a new one when `signature` is null. */
function SignatureEditor({
  signature,
  usedBy,
  confirm,
  onBack,
}: {
  signature: Signature | null;
  /** The addresses that add it by default. */
  usedBy: string[];
  confirm: (request: ConfirmRequest) => Promise<boolean>;
  onBack: () => void;
}) {
  const [name, setName] = useState(signature?.name ?? "");
  // Written with formatting and pictures: its text is the editor's, its HTML kept beside it.
  const [initial] = useState(() =>
    signature?.html ? coloursAsMarks(signature.html) : textSignature(signature?.body ?? ""),
  );
  const [html, setHtml] = useState(initial);
  const [body, setBody] = useState(signature?.body ?? "");
  const editor = useRef<Editor | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const changed = signature === null || name !== signature.name || html !== initial;
  const ready = name.trim() !== "" && body.trim() !== "" && body.length <= SIGNATURE_MAX && changed;
  const written = (now: string) => {
    setHtml(now);
    setBody(editor.current?.getText({ blockSeparator: "\n" }).trim() ?? "");
  };
  // Pictures pasted go in the signature itself, not attached to any message yet.
  const pictures = (files: File[]) => {
    for (const file of files) {
      if (file.size > PICTURE_MAX) {
        setError(t("“{name}” is too large for a signature: pictures there can be {size} KB at most.", { name: file.name, size: Math.round(PICTURE_MAX / 1024) }));
        continue;
      }
      void fileBase64(file).then((data) =>
        editor.current?.chain().focus().insertContent({ type: "image", attrs: { src: `data:${file.type};base64,${data}`, alt: "" } }).run(),
      );
    }
  };

  const run = (work: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    work().then(onBack, (failure: unknown) => {
      setError(String(failure));
      setBusy(false);
    });
  };
  const save = () =>
    run(() => saveSignature({ id: signature?.id ?? null, name: name.trim(), body, html: coloursAsStyles(html) }));
  const remove = () => {
    if (!signature) return;
    void confirm({
      title: t("Delete “{name}”?", { name: signature.name }),
      body:
        usedBy.length > 0
          ? tn(
              usedBy.length,
              "{addresses} adds it by default, and will add no signature. Messages already written keep theirs.",
              "{addresses} add it by default, and will add no signature. Messages already written keep theirs.",
              { addresses: usedBy.join(", ") },
            )
          : t("No address adds it by default. Messages already written keep theirs."),
      confirm: t("Delete"),
      danger: true,
    }).then((ok) => {
      if (ok) run(() => removeSignature(signature.id));
    });
  };

  return (
    <div className="signature-editor">
      <SubPageHead title={signature ? signature.name : t("New signature")} onBack={onBack} />
      <label className="signature-field">
        <span>{t("Name")}</span>
        <input
          className="input"
          value={name}
          maxLength={80}
          placeholder={t("Formal, Short, In English…")}
          onChange={(event) => setName(event.target.value)}
          autoFocus={signature === null}
        />
        <span className="muted small">{t("For you only: it names the signature in lists.")}</span>
      </label>
      <div className="signature-field">
        <span>{t("Text")}</span>
        <div className="signature-text">
          <Suspense fallback={<p className="muted small">{t("Opening the editor…")}</p>}>
            <RichEditor
              initial={initial}
              onChange={written}
              onSend={() => {
                if (ready && !busy) save();
              }}
              onImages={pictures}
              onEditor={(ready) => {
                editor.current = ready;
              }}
              disabled={busy}
              autofocus={false}
            />
          </Suspense>
        </div>
        <span className="signature-count muted small">
          {t("Bold, colours, links and pictures (pasted) go with it.")} {body.length} / {SIGNATURE_MAX}
        </span>
      </div>
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      <div className="signature-buttons">
        {signature && (
          <button type="button" className="btn small danger" onClick={remove} disabled={busy}>
            {t("Delete")}
          </button>
        )}
        <span className="spacer" />
        <button type="button" className="btn small" onClick={onBack} disabled={busy}>
          {t("Cancel")}
        </button>
        <button type="button" className="btn small primary" onClick={save} disabled={busy || !ready}>
          {t("Save")}
        </button>
      </div>
    </div>
  );
}
