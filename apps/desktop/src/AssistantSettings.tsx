import { useEffect, useState } from "react";
import { type AssistantKind, type AssistantSettings, assistantSettings, setAssistantSettings } from "./assistant";
import { assistantChanged } from "./AssistantDialog";
import { Option, Select } from "./SettingsControls";

type Provider = AssistantKind | "off";

/**
 * Settings › Assistant: off until a provider is chosen. With the person's own
 * key (or a model on this computer), the assistant summarises a conversation
 * or drafts a reply, only when asked, after showing what would go where.
 */
export function AssistantPage() {
  const [saved, setSaved] = useState<AssistantSettings | null>(null);
  const [provider, setProvider] = useState<Provider>("off");
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [key, setKey] = useState("");
  const [state, setState] = useState<"idle" | "saving" | "saved">("idle");
  const [error, setError] = useState<string | null>(null);

  const show = (settings: AssistantSettings) => {
    setSaved(settings);
    setProvider(settings.kind ?? "off");
    setBaseUrl(settings.kind === "openai_compatible" ? (settings.base_url ?? "") : "");
    setModel(settings.model ?? "");
    setKey("");
  };
  useEffect(() => {
    assistantSettings().then(show, (failure: unknown) => setError(String(failure)));
  }, []);

  const save = (choice: Provider) => {
    setState("saving");
    setError(null);
    setAssistantSettings(
      choice === "off"
        ? { kind: null }
        : {
            kind: choice,
            ...(choice === "openai_compatible" ? { base_url: baseUrl.trim() } : {}),
            ...(model.trim() ? { model: model.trim() } : {}),
            ...(key.trim() ? { api_key: key.trim() } : {}),
          },
    ).then(
      (settings) => {
        show(settings);
        setState("saved");
        assistantChanged();
      },
      (failure: unknown) => {
        setState("idle");
        setError(String(failure));
      },
    );
  };

  const anthropic = provider === "anthropic";
  const local = provider === "openai_compatible";
  const keptKey = saved?.has_key && saved.kind === provider;
  const changed =
    provider !== (saved?.kind ?? "off") ||
    key.trim() !== "" ||
    model.trim() !== (saved?.model ?? "") ||
    (local && baseUrl.trim() !== (saved?.base_url ?? ""));

  return (
    <>
      <h3>Assistant</h3>
      <p className="muted settings-note">
        With your own AI key, or a model on this computer, the assistant summarises a conversation or drafts a reply. Only
        when you ask, and only after showing what would be sent and where. A reply lands in a draft for you to check;
        nothing is sent by itself. Mail that looks like it carries a sign-in code is left out.
      </p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <Option icon="bolt" label="Provider" description="Off until you choose one. Turning it off forgets the key.">
        <Select
          label="Provider"
          value={provider}
          options={[
            { id: "off", label: "Off" },
            { id: "anthropic", label: "Anthropic" },
            { id: "openai_compatible", label: "OpenAI-compatible server" },
          ]}
          onChange={(next) => {
            setProvider(next);
            setState("idle");
            setError(null);
            if (next !== (saved?.kind ?? "off")) {
              setModel("");
              setKey("");
            }
            if (next === "off" && saved?.kind) save("off");
          }}
        />
      </Option>
      {provider !== "off" && (
        <div className="assistant-fields">
          {local && (
            <label>
              Server address
              <input
                className="input"
                value={baseUrl}
                placeholder="http://localhost:11434/v1"
                onChange={(event) => setBaseUrl(event.target.value)}
              />
              <span className="muted small">Up to /v1. http:// only for a server on this computer.</span>
            </label>
          )}
          <label>
            Model
            <input
              className="input"
              value={model}
              placeholder={anthropic ? "claude-opus-5-5" : "llama3.1"}
              onChange={(event) => setModel(event.target.value)}
            />
            {anthropic && <span className="muted small">Empty: claude-opus-5-5.</span>}
          </label>
          <label>
            {anthropic ? "API key" : "API key (if the server needs one)"}
            <input
              className="input"
              type="password"
              autoComplete="off"
              value={key}
              placeholder={keptKey ? "Kept in this computer's credential store; type to replace it" : anthropic ? "sk-ant-…" : ""}
              onChange={(event) => setKey(event.target.value)}
            />
            <span className="muted small">
              {anthropic
                ? "From the Anthropic Console. Kept in this computer's credential store, and sent only to that server."
                : "Kept in this computer's credential store, and sent only to that server."}
            </span>
          </label>
          <div className="assistant-save">
            <button type="button" className="btn primary" onClick={() => save(provider)} disabled={!changed || state === "saving"}>
              {state === "saving" ? "Saving…" : "Save"}
            </button>
            <span className="muted small" aria-live="polite">
              {state === "saved" && !changed ? "Saved. Summarise and Draft reply are in each message's More menu." : null}
            </span>
          </div>
        </div>
      )}
    </>
  );
}
