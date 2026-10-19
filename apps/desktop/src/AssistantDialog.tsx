import { useEffect, useRef, useState } from "react";
import {
  type AssistantPreview,
  type AssistantResult,
  type AssistantTask,
  assistantSettings,
  describePreview,
  previewAssistant,
  runAssistant,
} from "./assistant";
import type { Draft } from "./compose";
import { useDialogFocus } from "./dialogFocus";
import { Icon } from "./icons";
import { useShortcuts } from "./shortcuts";

/** What the person asked the assistant for, about which message. */
export interface AssistantJob {
  messageId: number;
  kind: AssistantTask["kind"];
}

const CHANGED = "regards-assistant-changed";

/** Settings changed the assistant: whoever shows its buttons looks again. */
export const assistantChanged = () => window.dispatchEvent(new Event(CHANGED));

/** Whether a provider is chosen, so the assistant's actions are offered. */
export function useAssistantOn(): boolean {
  const [on, setOn] = useState(false);
  useEffect(() => {
    const look = () =>
      assistantSettings().then(
        (settings) => setOn(settings.kind !== null),
        () => setOn(false),
      );
    look();
    window.addEventListener(CHANGED, look);
    return () => window.removeEventListener(CHANGED, look);
  }, []);
  return on;
}

/**
 * Asking the assistant, in three steps: what would be sent and where (nothing
 * goes before the person says so), working (it can take up to two minutes),
 * then the summary to read, or the reply's draft opened in the composer.
 */
export function AssistantDialog({
  job,
  onClose,
  onDraft,
}: {
  job: AssistantJob;
  onClose: () => void;
  /** A reply was drafted: open it to review. It is never sent by itself. */
  onDraft: (draft: Draft) => void;
}) {
  const box = useRef<HTMLElement>(null);
  useDialogFocus(box);
  useShortcuts({ Escape: onClose }, { modal: true });
  const reply = job.kind === "reply";
  const [instructions, setInstructions] = useState("");
  const [preview, setPreview] = useState<AssistantPreview | null>(null);
  const [step, setStep] = useState<"ask" | "working" | "done">("ask");
  const [result, setResult] = useState<AssistantResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  // Closed while it worked: the answer is dropped. Set on each mount (React may mount twice).
  const closed = useRef(false);
  useEffect(() => {
    closed.current = false;
    return () => {
      closed.current = true;
    };
  }, []);

  const task = (): AssistantTask => (reply ? { kind: "reply", instructions: instructions.trim() } : { kind: "summary" });

  useEffect(() => {
    previewAssistant(job.messageId, reply ? { kind: "reply", instructions: "" } : { kind: "summary" }).then(setPreview, (failure: unknown) =>
      setError(String(failure)),
    );
  }, [job.messageId, reply]);

  const run = () => {
    setError(null);
    setStep("working");
    runAssistant(job.messageId, task()).then(
      (done) => {
        if (closed.current) return;
        if (done.draft) {
          onDraft(done.draft);
          onClose();
          return;
        }
        setResult(done);
        setStep("done");
      },
      (failure: unknown) => {
        if (closed.current) return;
        setError(String(failure));
        setStep("ask");
      },
    );
  };
  const copy = () => {
    if (!result) return;
    navigator.clipboard?.writeText(result.text).then(
      () => setCopied(true),
      () => {},
    );
  };

  return (
    <div className="backdrop" onMouseDown={onClose}>
      <section
        ref={box}
        tabIndex={-1}
        className="dialog assistant-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="assistant-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <h2 id="assistant-title">
          <Icon name="bolt" size={18} />
          {reply ? "Draft a reply" : "Summarise the conversation"}
        </h2>

        {step === "ask" && (
          <>
            {reply && (
              <label className="assistant-instructions">
                What should it say?
                <textarea
                  className="input"
                  rows={3}
                  value={instructions}
                  placeholder="Say yes, but Friday suits me better."
                  onChange={(event) => setInstructions(event.target.value)}
                  autoFocus
                />
                <span className="muted small">Optional. The draft opens for you to check; nothing is sent.</span>
              </label>
            )}
            <p className="assistant-preview" aria-live="polite">
              {preview ? describePreview(preview) : error ? null : "Looking at what would be sent…"}
            </p>
            {error && (
              <p className="error" role="alert">
                {error}
              </p>
            )}
            <div className="dialog-actions">
              <button type="button" className="btn" onClick={onClose}>
                Cancel
              </button>
              <button type="button" className="btn primary" onClick={run} disabled={!preview}>
                {preview ? `Send to ${preview.destination}` : reply ? "Write the draft" : "Summarise"}
              </button>
            </div>
          </>
        )}

        {step === "working" && (
          <>
            <p className="assistant-working" role="status">
              <span className="assistant-spinner" aria-hidden="true" />
              Asking {preview?.model ?? "the model"}… This can take up to two minutes.
            </p>
            <div className="dialog-actions">
              <button type="button" className="btn" onClick={onClose}>
                Close
              </button>
            </div>
          </>
        )}

        {step === "done" && result && (
          <>
            <div className="assistant-result" tabIndex={0} aria-label="Summary">
              {result.text}
            </div>
            <p className="muted small">Written by {preview?.model ?? "a model"}: check it against the messages.</p>
            <div className="dialog-actions">
              <button type="button" className="btn" onClick={copy}>
                <Icon name={copied ? "check" : "copy"} size={14} />
                {copied ? "Copied" : "Copy"}
              </button>
              <button type="button" className="btn primary" onClick={onClose}>
                Close
              </button>
            </div>
          </>
        )}
      </section>
    </div>
  );
}
