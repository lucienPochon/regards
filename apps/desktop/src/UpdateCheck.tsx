import { useCallback, useEffect, useState } from "react";
import { Icon } from "./icons";
import {
  type Update,
  type UpdateStatus,
  checkForUpdate,
  onUpdateAvailable,
  openUpdate,
  setUpdateChecks,
  updateStatus,
} from "./updates";
import { Toggle } from "./SettingsControls";

/**
 * Whether Regards checks for updates, the running version and any newer one,
 * kept current when a daily check finds one.
 */
export function useUpdateStatus() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const refresh = useCallback(() => {
    updateStatus().then(setStatus, () => {});
  }, []);
  useEffect(() => {
    refresh();
    const stop = onUpdateAvailable(() => refresh());
    return () => void stop.then((unlisten) => unlisten()).catch(() => {});
  }, [refresh]);
  return { status, setStatus, refresh };
}

const time = (at: number) => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

/** Settings › About: the version, checking automatically or not, Check now, and Download. */
export function AboutUpdates() {
  const { status, setStatus } = useUpdateStatus();
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checked, setChecked] = useState<{ at: number; found: Update | null } | null>(null);
  if (!status) return null;

  const choose = (on: boolean) => {
    setError(null);
    setUpdateChecks(on).then(
      () => setStatus({ ...status, checks: on }),
      (failure: unknown) => setError(String(failure)),
    );
  };
  const checkNow = () => {
    setChecking(true);
    setError(null);
    checkForUpdate()
      .then(
        (found) => {
          setChecked({ at: Date.now(), found });
          setStatus({ ...status, available: found ?? status.available, checked_at: Date.now() });
        },
        (failure: unknown) => setError(String(failure)),
      )
      .finally(() => setChecking(false));
  };
  const available = status.available;
  const lastChecked = checked?.at ?? status.checked_at;

  return (
    <section className="about-updates" aria-label="Updates">
      <h4 className="settings-subhead">Updates</h4>
      <p className="about-version">
        Version <strong>{status.current}</strong>
      </p>
      <div className="option">
        <span className="option-icon">
          <Icon name="refresh" size={18} />
        </span>
        <span className="option-text">
          <strong>Check for updates automatically</strong>
          <span>Once a day, Regards asks GitHub for the latest version. GitHub sees this computer's address; nothing else is sent.</span>
        </span>
        <span className="option-control">
          <Toggle label="Check for updates automatically" on={status.checks === true} onChange={choose} />
        </span>
      </div>
      <div className="update-now">
        <button type="button" className="btn" onClick={checkNow} disabled={checking}>
          <Icon name="refresh" size={15} />
          {checking ? "Checking…" : "Check now"}
        </button>
        <span className="muted" aria-live="polite">
          {error
            ? null
            : available
              ? null
              : checked
                ? `You have the latest version (checked at ${time(checked.at)}).`
                : lastChecked
                  ? `Last checked at ${time(lastChecked)}.`
                  : null}
        </span>
      </div>
      {error && <p className="error">{error}</p>}
      {available && (
        <p className="update-available">
          <Icon name="check" size={15} />
          <span>
            Regards {available.version} is available
            {available.published_at ? `, released ${new Date(available.published_at).toLocaleDateString()}` : ""}.
          </span>
          <button type="button" className="btn small primary" onClick={() => void openUpdate().catch((failure: unknown) => setError(String(failure)))}>
            Download
          </button>
        </p>
      )}
    </section>
  );
}

/**
 * Asked once, after the first account: checking for updates tells GitHub
 * this computer's address, so it stays off until the person says yes.
 */
export function AskUpdates({ onAnswer }: { onAnswer: (on: boolean) => void }) {
  const [busy, setBusy] = useState(false);
  const answer = (on: boolean) => {
    setBusy(true);
    setUpdateChecks(on)
      .then(() => onAnswer(on), () => onAnswer(on))
      .finally(() => setBusy(false));
  };
  return (
    <div className="toast ask-updates" role="dialog" aria-label="Check for updates">
      <Icon name="refresh" size={16} />
      <span>
        <strong>Check for updates once a day?</strong>
        <span className="muted">Regards asks GitHub for the latest version, and GitHub sees this computer's address.</span>
      </span>
      <button type="button" onClick={() => answer(false)} disabled={busy}>
        No
      </button>
      <button type="button" className="primary" onClick={() => answer(true)} disabled={busy}>
        Yes
      </button>
    </div>
  );
}
