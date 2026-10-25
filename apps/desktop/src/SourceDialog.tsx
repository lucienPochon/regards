import { useEffect, useRef, useState } from "react";
import { type MessageSource, messageSource } from "./accounts";
import { useDialogFocus } from "./dialogFocus";
import { fileSize } from "./format";
import { Icon } from "./icons";
import { useShortcuts } from "./shortcuts";
import { t } from "./i18n";

/**
 * "See the original message": the message as it came, headers and all, as
 * plain text. Never shown as HTML: it is the raw message, whatever is in it.
 */
export function SourceDialog({ messageId, subject, onClose }: { messageId: number; subject: string; onClose: () => void }) {
  const box = useRef<HTMLElement>(null);
  useDialogFocus(box);
  useShortcuts({ Escape: onClose }, { modal: true });
  const [source, setSource] = useState<MessageSource | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    messageSource(messageId).then(setSource, (failure: unknown) => setError(String(failure)));
  }, [messageId]);

  const copy = () => {
    if (!source) return;
    navigator.clipboard?.writeText(source.text).then(
      () => setCopied(true),
      () => setError(t("Copying didn't work here: select the text and copy it.")),
    );
  };

  return (
    <div className="backdrop" onMouseDown={onClose}>
      <section
        ref={box}
        tabIndex={-1}
        className="dialog source-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="source-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="source-head">
          <h2 id="source-title">{t("Original message")}</h2>
          <span className="muted small">
            {source
              ? source.truncated
                ? t("The first 1 MB of {size}", { size: fileSize(source.size) })
                : fileSize(source.size)
              : null}
          </span>
        </header>
        <p className="muted small source-subject">{subject}</p>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        {!source && !error && <p className="muted">{t("Fetching the original…")}</p>}
        {source && (
          <pre className="source-text" tabIndex={0} aria-label={t("The message as it came")}>
            {source.text}
          </pre>
        )}
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={copy} disabled={!source}>
            <Icon name={copied ? "check" : "copy"} size={14} />
            {copied ? t("Copied") : t("Copy")}
          </button>
          <button type="button" className="btn primary" onClick={onClose}>
            {t("Close")}
          </button>
        </div>
      </section>
    </div>
  );
}
