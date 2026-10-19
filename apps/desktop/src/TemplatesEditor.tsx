import { type FormEvent, useCallback, useEffect, useState } from "react";
import { Icon } from "./icons";
import { type Template, listTemplates, removeTemplate, saveTemplate } from "./templates";

/**
 * Templates (plan §5.9): text to start a message with, by name. Written as
 * plain text here; one saved from the composer keeps its formatting.
 */
export function TemplatesEditor() {
  const [templates, setTemplates] = useState<Template[] | null>(null);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => {
    listTemplates().then(setTemplates, (failure: unknown) => setError(String(failure)));
  }, []);
  useEffect(reload, [reload]);

  if (templates === null) return error ? <p className="error small">{error}</p> : null;
  return (
    <div className="account-form">
      {error && <p className="error small">{error}</p>}
      {templates.length === 0 && !adding && (
        <p className="muted small">No templates yet. They start a message with text you write often.</p>
      )}
      {templates.map((template) => (
        <TemplateForm key={template.id} template={template} onDone={reload} onError={setError} />
      ))}
      {adding ? (
        <TemplateForm
          template={null}
          onDone={() => {
            setAdding(false);
            reload();
          }}
          onCancel={() => setAdding(false)}
          onError={setError}
        />
      ) : (
        <div>
          <button type="button" className="btn small" onClick={() => setAdding(true)}>
            <Icon name="plus" size={15} />
            New template
          </button>
        </div>
      )}
    </div>
  );
}

function TemplateForm({
  template,
  onDone,
  onCancel,
  onError,
}: {
  /** None: a new template. */
  template: Template | null;
  onDone: () => void;
  onCancel?: () => void;
  onError: (error: string | null) => void;
}) {
  const [name, setName] = useState(template?.name ?? "");
  const [subject, setSubject] = useState(template?.subject ?? "");
  const [body, setBody] = useState(template?.body ?? "");
  const [busy, setBusy] = useState(false);
  // A formatted template keeps its formatting until its text is changed here.
  const changed =
    template === null || name !== template.name || subject !== template.subject || body !== template.body;

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    onError(null);
    try {
      await action();
      onDone();
    } catch (failure) {
      onError(String(failure));
    } finally {
      setBusy(false);
    }
  };
  const save = (event: FormEvent) => {
    event.preventDefault();
    const html = template && body === template.body ? template.html : null;
    void run(() => saveTemplate({ id: template?.id ?? null, name, subject, body, html }));
  };

  return (
    <form className="template-form" onSubmit={save}>
      <fieldset>
        <legend>{template ? template.name : "A new template"}</legend>
        <div className="form-row grow">
          <label>
            Name
            <input className="input" required value={name} onChange={(e) => setName(e.target.value)} placeholder="Booking reply" />
          </label>
          <label>
            Subject (optional)
            <input className="input" value={subject} onChange={(e) => setSubject(e.target.value)} placeholder="Used when a message has none" />
          </label>
        </div>
        <label className="grow">
          Text
          <textarea className="input" rows={4} value={body} onChange={(e) => setBody(e.target.value)} />
        </label>
        <div className="dialog-actions">
          {onCancel && (
            <button type="button" className="btn small" onClick={onCancel} disabled={busy}>
              Cancel
            </button>
          )}
          {template && (
            <button type="button" className="btn small danger" onClick={() => void run(() => removeTemplate(template.id))} disabled={busy}>
              Remove
            </button>
          )}
          <button type="submit" className="btn small primary" disabled={busy || !changed}>
            {template ? "Save" : "Add"}
          </button>
        </div>
      </fieldset>
    </form>
  );
}
