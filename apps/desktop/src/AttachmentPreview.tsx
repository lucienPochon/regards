import { useEffect, useRef, useState } from "react";
import type { Attachment } from "./accounts";
import { type AttachmentPreview, imageSource, openAttachment, previewAttachment, saveAttachment } from "./attachments";
import { Confirm, type ConfirmRequest } from "./Confirm";
import { useDialogFocus } from "./dialogFocus";
import { fileKind, fileSize } from "./format";
import { Icon } from "./icons";
import { useShortcuts } from "./shortcuts";

/** An attachment and its place in the message's list (inline ones count). */
export interface Placed {
  index: number;
  attachment: Attachment;
}

/** Previews fetched lately: the tiles' thumbnails and the viewer share them. */
const previews = new Map<string, Promise<AttachmentPreview>>();
const KEPT = 32;

export function loadPreview(messageId: number, index: number): Promise<AttachmentPreview> {
  const key = `${messageId}:${index}`;
  let found = previews.get(key);
  if (!found) {
    found = previewAttachment(messageId, index);
    found.catch(() => previews.delete(key));
    previews.set(key, found);
    const oldest = previews.keys().next().value;
    if (previews.size > KEPT && oldest !== undefined) previews.delete(oldest);
  }
  return found;
}

export const SAVE_ONLY_TEXT =
  "This file could run a program, so Regards doesn't open it. Save it, and open it yourself only if you trust the sender.";
const WARN_TEXT =
  "Files like this can run scripts or macros, or hold other files. Open it only if you expected it from this sender.";

type Shown = { state: "none" } | { state: "loading" } | { state: "ready"; preview: AttachmentPreview } | { state: "failed"; message: string };

/**
 * The attachments of a message over the whole window, one at a time: images
 * and text shown here, anything else described, with Open and Save as…
 * ←/→ move between them, Esc closes and focus goes back to the tile.
 */
export function AttachmentViewer({
  messageId,
  files,
  start,
  onClose,
}: {
  messageId: number;
  files: Placed[];
  start: number;
  onClose: () => void;
}) {
  const box = useRef<HTMLElement>(null);
  useDialogFocus(box);
  const [at, setAt] = useState(start);
  const [shown, setShown] = useState<Shown>({ state: "loading" });
  const [actual, setActual] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null);
  const file = files[Math.min(at, files.length - 1)];
  const { attachment, index } = file;
  const name = attachment.name ?? "(unnamed)";
  const kind = fileKind(attachment.name, attachment.mime);

  const go = (step: number) => {
    if (files.length > 1) setAt((now) => (now + step + files.length) % files.length);
  };
  // Space closes it, as it opened it (AttachmentTiles).
  useShortcuts({ Escape: onClose, " ": onClose, ArrowLeft: () => go(-1), ArrowRight: () => go(1) }, { modal: true });

  useEffect(() => {
    setActual(false);
    setStatus(null);
    setError(null);
    if (!attachment.preview) {
      setShown({ state: "none" });
      return;
    }
    let live = true;
    setShown({ state: "loading" });
    loadPreview(messageId, index).then(
      (preview) => live && setShown({ state: "ready", preview }),
      (failure: unknown) => live && setShown({ state: "failed", message: String(failure) }),
    );
    return () => {
      live = false;
    };
  }, [messageId, index, attachment.preview]);

  const save = () => {
    setError(null);
    saveAttachment(messageId, index, name).then(
      (saved) => saved && setStatus(`Saved as “${saved}”`),
      (failure: unknown) => setError(String(failure)),
    );
  };
  const reallyOpen = () => {
    setError(null);
    openAttachment(messageId, index).catch((failure: unknown) => setError(String(failure)));
  };
  const open = () => {
    if (attachment.risk === "warn") setConfirm({ title: `Open “${name}”?`, body: WARN_TEXT, confirm: "Open" });
    else reallyOpen();
  };

  const image = shown.state === "ready" && shown.preview.kind === "image" ? shown.preview : null;
  let stage;
  if (shown.state === "loading") {
    stage = <p className="viewer-note">Loading…</p>;
  } else if (image) {
    stage = <img className={actual ? "actual" : undefined} src={imageSource(image)} alt={name} decoding="async" />;
  } else if (shown.state === "ready" && shown.preview.kind === "text") {
    stage = (
      <div className="viewer-text">
        <pre tabIndex={0} aria-label={name}>
          {shown.preview.text}
        </pre>
        {shown.preview.truncated && <p className="viewer-note">Only the first megabyte is shown. Open or save the file to see it all.</p>}
      </div>
    );
  } else {
    const why =
      shown.state === "failed"
        ? shown.message
        : attachment.risk === "save_only"
          ? SAVE_ONLY_TEXT
          : attachment.risk === "warn"
            ? WARN_TEXT
            : "Regards can't show this kind of file. Open it with its program, or save it.";
    stage = (
      <div className="viewer-none">
        <span className="viewer-face" aria-hidden="true">
          {attachment.risk === "save_only" ? <Icon name="alert" size={40} /> : kind}
        </span>
        <p>{why}</p>
      </div>
    );
  }

  return (
    <>
      <div className="viewer-layer">
        <section ref={box} tabIndex={-1} className="viewer" role="dialog" aria-modal="true" aria-labelledby="viewer-title">
          <header className="viewer-head">
            <div className="viewer-title">
              <h2 id="viewer-title" title={name}>
                {name}
              </h2>
              <span className="viewer-meta">
                {kind} · {fileSize(attachment.size)}
                {files.length > 1 && (
                  <span aria-live="polite">
                    {" "}
                    · {at + 1} of {files.length}
                  </span>
                )}
              </span>
            </div>
            <span className="viewer-status" role="status">
              {status}
            </span>
            {image && (
              <button
                type="button"
                className="viewer-btn square"
                aria-label={actual ? "Fit to the window" : "Actual size"}
                title={actual ? "Fit to the window" : "Actual size"}
                aria-pressed={actual}
                onClick={() => setActual(!actual)}
              >
                <Icon name={actual ? "shrink" : "expand"} size={16} />
              </button>
            )}
            {attachment.risk !== "save_only" && (
              <button type="button" className="viewer-btn" onClick={open}>
                <Icon name="external" size={15} />
                Open
              </button>
            )}
            <button type="button" className="viewer-btn" onClick={save}>
              <Icon name="download" size={15} />
              Save as…
            </button>
            <button type="button" className="viewer-btn square" aria-label="Close" title="Close (Esc)" onClick={onClose}>
              <Icon name="close" size={18} />
            </button>
          </header>
          {error && (
            <p className="viewer-error" role="alert">
              {error}
            </p>
          )}
          <div className="viewer-stage" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
            {stage}
            {files.length > 1 && (
              <>
                <button
                  type="button"
                  className="viewer-nav previous"
                  aria-label="Previous attachment"
                  title="Previous (←)"
                  onClick={() => go(-1)}
                >
                  <Icon name="chevronLeft" size={22} />
                </button>
                <button type="button" className="viewer-nav next" aria-label="Next attachment" title="Next (→)" onClick={() => go(1)}>
                  <Icon name="chevronRight" size={22} />
                </button>
              </>
            )}
          </div>
        </section>
      </div>
      {confirm && (
        <Confirm
          request={confirm}
          onAnswer={(ok) => {
            setConfirm(null);
            if (ok) reallyOpen();
          }}
        />
      )}
    </>
  );
}
