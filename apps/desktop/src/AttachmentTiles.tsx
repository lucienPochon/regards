import { useEffect, useState } from "react";
import type { Attachment } from "./accounts";
import { imageSource, saveAllAttachments, saveAttachment } from "./attachments";
import { AttachmentViewer, type Placed, loadPreview } from "./AttachmentPreview";
import { fileKind, fileSize } from "./format";
import { Icon } from "./icons";

/** Images up to this size get a thumbnail. */
const THUMBNAIL_MAX = 3 * 1024 * 1024;

/**
 * A message's attached files, below its text: a tile each, which opens the
 * viewer, with Save as… on hover; "Save all" in the header. Images shown
 * inside the message are left out.
 */
export function AttachmentTiles({ messageId, attachments }: { messageId: number; attachments: Attachment[] }) {
  const files: Placed[] = attachments
    .map((attachment, index) => ({ index, attachment }))
    .filter((file) => !file.attachment.inline);
  const [viewing, setViewing] = useState<number | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  if (files.length === 0) return null;
  const total = files.reduce((sum, file) => sum + file.attachment.size, 0);

  const failed = (failure: unknown) => setError(String(failure));
  const saveAll = () => {
    setError(null);
    saveAllAttachments(messageId).then((saved) => {
      if (saved) setStatus(saved.length === 1 ? `Saved as “${saved[0]}”` : `Saved ${saved.length} files`);
    }, failed);
  };
  const saveOne = (file: Placed) => {
    setError(null);
    saveAttachment(messageId, file.index, file.attachment.name ?? "attachment").then(
      (saved) => saved && setStatus(`Saved as “${saved}”`),
      failed,
    );
  };

  return (
    <div className="attachments">
      <p className="attachments-head">
        <Icon name="paperclip" size={15} />
        <strong>
          {files.length} {files.length === 1 ? "attachment" : "attachments"}
        </strong>
        <span className="muted">{fileSize(total)}</span>
        <button type="button" className="link" onClick={saveAll}>
          {files.length === 1 ? "Save" : "Save all"}
        </button>
        <span className="muted small" role="status">
          {status}
        </span>
      </p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <ul className="tiles">
        {files.map((file, at) => (
          <Tile
            key={file.index}
            messageId={messageId}
            file={file}
            onOpen={() => setViewing(at)}
            onSave={() => saveOne(file)}
          />
        ))}
      </ul>
      {viewing !== null && (
        <AttachmentViewer messageId={messageId} files={files} start={viewing} onClose={() => setViewing(null)} />
      )}
    </div>
  );
}

function Tile({
  messageId,
  file,
  onOpen,
  onSave,
}: {
  messageId: number;
  file: Placed;
  onOpen: () => void;
  onSave: () => void;
}) {
  const { attachment, index } = file;
  const name = attachment.name ?? "(unnamed)";
  const kind = fileKind(attachment.name, attachment.mime);
  const wanted = attachment.preview === "image" && attachment.size <= THUMBNAIL_MAX;
  const [thumbnail, setThumbnail] = useState<string | null>(null);
  useEffect(() => {
    if (!wanted) return;
    let live = true;
    loadPreview(messageId, index).then(
      (preview) => live && preview.kind === "image" && setThumbnail(imageSource(preview)),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [messageId, index, wanted]);
  const saveOnly = attachment.risk === "save_only";

  return (
    <li className="tile">
      <button
        type="button"
        className="tile-open"
        title={name}
        aria-label={`${name}, ${kind}, ${fileSize(attachment.size)}${saveOnly ? ", can only be saved" : ""}`}
        onClick={onOpen}
      >
        <span className={thumbnail ? "tile-face thumbnail" : "tile-face"}>
          {thumbnail ? <img src={thumbnail} alt="" /> : saveOnly ? <Icon name="alert" size={26} /> : kind}
        </span>
        <span className="tile-name">{name}</span>
        <span className="tile-size">
          {kind} · {fileSize(attachment.size)}
        </span>
      </button>
      <button type="button" className="tile-save" aria-label={`Save ${name}`} title="Save as…" onClick={onSave}>
        <Icon name="download" size={13} />
      </button>
    </li>
  );
}
