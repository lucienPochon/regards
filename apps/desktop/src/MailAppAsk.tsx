import { useEffect, useState } from "react";
import { Icon } from "./icons";
import { mailAppStatus, makeMailApp } from "./links";
import { getPrefs, setPrefs } from "./prefs";
import { t } from "./i18n";

/** How long a toast that only tells something stays. */
export const TOLD_MS = 5000;

type Phase = "checking" | "ask" | "mac" | "waiting" | "done" | "later" | "error" | "gone";

/**
 * Asked once in the main window, after the first account: make Regards the
 * default mail app? Never when it already is. On Windows, Default apps opens
 * on Regards' page to choose it, and back in Regards this says whether it
 * took. On macOS, Mail's settings choose: this says where. Settings › General
 * keeps the same choice for later.
 */
export function AskMailApp({ hidden }: { hidden: boolean }) {
  const [phase, setPhase] = useState<Phase>(() => (getPrefs().mailAppAsked ? "gone" : "checking"));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (phase !== "checking") return;
    let live = true;
    mailAppStatus().then(
      (status) => live && setPhase(status.registered ? "gone" : status.can_register ? "ask" : "mac"),
      () => live && setPhase("gone"),
    );
    return () => {
      live = false;
    };
  }, [phase]);

  // Back from Default apps: whether the person chose Regards there.
  useEffect(() => {
    if (phase !== "waiting") return;
    const check = () => {
      mailAppStatus().then(
        (status) => status.registered && setPhase("done"),
        () => {},
      );
    };
    window.addEventListener("focus", check);
    return () => window.removeEventListener("focus", check);
  }, [phase]);

  useEffect(() => {
    if (phase !== "done" && phase !== "later") return;
    const timer = window.setTimeout(() => setPhase("gone"), TOLD_MS);
    return () => window.clearTimeout(timer);
  }, [phase]);

  const answered = () => setPrefs({ mailAppAsked: true });
  const yes = () => {
    answered();
    makeMailApp().then(
      () =>
        mailAppStatus().then(
          // Windows waits for the person's choice in Default apps; elsewhere it is made at once.
          (status) => setPhase(status.registered ? "done" : /Win/.test(navigator.platform) ? "waiting" : "gone"),
          () => setPhase("gone"),
        ),
      (failure: unknown) => {
        setError(String(failure));
        setPhase("error");
      },
    );
  };

  if (hidden || phase === "checking" || phase === "gone") return null;

  if (phase === "ask" || phase === "mac") {
    return (
      <div className="toast ask-mail-app" role="region" aria-label={t("Default mail app")}>
        <Icon name="mail" size={16} />
        <span>
          <strong>{t("Make Regards your default mail app?")}</strong>
          <span className="muted">
            {phase === "ask"
              ? t("Email addresses you click elsewhere will open here.")
              : t("Choose it in Mail, under Settings › General.")}
          </span>
        </span>
        {phase === "ask" ? (
          <>
            <button
              type="button"
              onClick={() => {
                answered();
                setPhase("later");
              }}
            >
              {t("Not now")}
            </button>
            <button type="button" className="primary" onClick={yes}>
              {t("Yes")}
            </button>
          </>
        ) : (
          <button
            type="button"
            className="primary"
            onClick={() => {
              answered();
              setPhase("gone");
            }}
          >
            {t("Got it")}
          </button>
        )}
      </div>
    );
  }

  const dismiss = (
    <button type="button" onClick={() => setPhase("gone")}>
      {t("Dismiss")}
    </button>
  );
  if (phase === "error") {
    return (
      <div className="toast error" role="alert">
        <Icon name="alert" size={16} />
        <span>{error}</span>
        {dismiss}
      </div>
    );
  }
  return (
    <div className="toast">
      <Icon name={phase === "waiting" ? "clock" : phase === "done" ? "check" : "info"} size={16} />
      <span>
        {phase === "waiting"
          ? t("In Default apps, which just opened, choose Regards for MAILTO.")
          : phase === "done"
            ? t("Done: Regards is your default mail app.")
            : t("OK. It’s in Settings › General if you change your mind.")}
      </span>
      {phase === "waiting" && dismiss}
    </div>
  );
}
