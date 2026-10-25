import { useCallback, useEffect, useRef, useState } from "react";
import { type AccountSummary, type Provider, cancelOauth, finishOauth, startOauth } from "./accounts";
import { Icon } from "./icons";
import { t } from "./i18n";

/** What each provider covers, for its button. */
export const PROVIDERS: Record<Provider, { label: string; covers: string }> = {
  microsoft: { label: t("Microsoft"), covers: "Outlook.com, Hotmail and Microsoft 365" },
  google: { label: t("Google"), covers: "Gmail and Google Workspace" },
};

export interface OauthFlow {
  id: string;
  url: string;
  provider: Provider;
}

/**
 * Signing in with a provider (ADR-0018): the daemon opens its page in the
 * browser, and this waits for the person to come back. Leaving the screen
 * while it waits gives the sign-in up.
 */
export function useOauth(onSignedIn: (account: AccountSummary) => void) {
  const [flow, setFlow] = useState<OauthFlow | null>(null);
  const [error, setError] = useState<string | null>(null);
  const current = useRef<string | null>(null);
  const cancelled = useRef<string | null>(null);
  const signedIn = useRef(onSignedIn);
  signedIn.current = onSignedIn;

  const start = useCallback(async (provider: Provider, email: string | null) => {
    setError(null);
    let started;
    try {
      started = await startOauth(provider, email);
    } catch (failure) {
      setError(String(failure));
      return;
    }
    current.current = started.flow_id;
    setFlow({ id: started.flow_id, url: started.url, provider });
    try {
      signedIn.current(await finishOauth(started.flow_id));
    } catch (failure) {
      // A sign-in the person gave up says nothing more.
      if (cancelled.current !== started.flow_id) setError(String(failure));
    } finally {
      if (current.current === started.flow_id) {
        current.current = null;
        setFlow(null);
      }
    }
  }, []);

  const cancel = useCallback(() => {
    const id = current.current;
    if (!id) return;
    cancelled.current = id;
    void cancelOauth(id).catch(() => {});
  }, []);

  useEffect(
    () => () => {
      const id = current.current;
      if (id) {
        cancelled.current = id;
        void cancelOauth(id).catch(() => {});
      }
    },
    [],
  );

  return { flow, error, start, cancel };
}

/** While the person signs in in their browser: what to do, the page's address if no window opened, and Cancel. */
export function OauthWaiting({ flow, onCancel }: { flow: OauthFlow; onCancel: () => void }) {
  const [copied, setCopied] = useState(false);
  const field = useRef<HTMLInputElement>(null);
  const copy = () => {
    navigator.clipboard?.writeText(flow.url).then(
      () => setCopied(true),
      () => field.current?.select(),
    );
  };
  return (
    <div className="oauth-waiting" role="status">
      <span className="spinner" aria-hidden="true" />
      <strong>{t("Waiting for your browser…")}</strong>
      <p className="muted">
        {t("Sign in to")} {PROVIDERS[flow.provider].label} {t("in the browser window that opened, then come back here.")}
      </p>
      <div className="oauth-url">
        <label htmlFor="oauth-url">{t("If no window opened, open this address in your browser:")}</label>
        <span>
          <input id="oauth-url" ref={field} className="input" readOnly value={flow.url} onFocus={(event) => event.target.select()} />
          <button type="button" className="btn small" onClick={copy}>
            <Icon name={copied ? "check" : "copy"} size={14} />
            {copied ? t("Copied") : t("Copy")}
          </button>
        </span>
      </div>
      <button type="button" className="btn" onClick={onCancel}>
        {t("Cancel")}
      </button>
    </div>
  );
}
