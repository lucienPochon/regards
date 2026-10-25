import { useState } from "react";
import type { ConfirmRequest } from "./Confirm";
import { Icon } from "./icons";
import { eraseEverything } from "./support";
import { t } from "./i18n";

/**
 * Support › This computer › Remove everything: what Regards keeps here,
 * gone in one step after asking, naming what goes. Then Regards quits, and
 * its next start is a first run. Mail on the servers is not touched.
 */
export function EraseEverything({ confirm }: { confirm: (request: ConfirmRequest) => Promise<boolean> }) {
  const [state, setState] = useState<"idle" | "removing" | "removed">("idle");
  const [error, setError] = useState<string | null>(null);

  const remove = async () => {
    const ok = await confirm({
      title: t("Remove everything from this computer?"),
      body:
        t("Your passwords, sign-ins and AI key leave this computer's credential store. Then all mail, drafts and the Outbox go ") +
        "(messages waiting there are not sent), with the settings and the logs, and Regards no longer starts at login. " +
        t("Regards then closes; next time it starts as new. Your mail on the servers stays as it is."),
      confirm: t("Remove everything"),
      danger: true,
    });
    if (!ok) return;
    setState("removing");
    setError(null);
    // The app's own choices kept in this window (layout, theme, list settings) go too.
    try {
      window.localStorage.clear();
    } catch {
      // Storage the window can't reach has nothing of ours in it.
    }
    eraseEverything().then(
      // The app quits, so this is reached only where it doesn't (the preview).
      () => setState("removed"),
      (failure: unknown) => {
        setState("idle");
        setError(String(failure));
      },
    );
  };

  return (
    <>
      <div className="option">
        <span className="option-icon">
          <Icon name="trash" size={18} />
        </span>
        <span className="option-text">
          <strong>{t("Remove everything from this computer")}</strong>
          <span role="status">
            {state === "removing"
              ? t("Removing… Regards closes when it is done.")
              : state === "removed"
                ? t("Removed. Regards closes now.")
                : t("Passwords and sign-ins, mail, drafts, the Outbox, settings and logs. Your mail on the servers stays.")}
          </span>
        </span>
        <span className="option-control">
          <button type="button" className="btn small danger" onClick={() => void remove()} disabled={state !== "idle"}>
            {t("Remove everything…")}
          </button>
        </span>
      </div>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </>
  );
}
