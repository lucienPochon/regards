import { useState } from "react";
import { type AccountSummary, resyncAccount } from "./accounts";
import type { ConfirmRequest } from "./Confirm";
import { Icon } from "./icons";
import { Select } from "./SettingsControls";

/**
 * Support › Troubleshooting › Download an account again: for when an
 * account's mail looks wrong here. Its mail on this computer is dropped and
 * fetched again, newest first; the server is not touched.
 */
export function DownloadAgain({
  accounts,
  confirm,
}: {
  accounts: AccountSummary[];
  confirm: (request: ConfirmRequest) => Promise<boolean>;
}) {
  const [accountId, setAccountId] = useState(accounts[0]?.id ?? "");
  const [state, setState] = useState<"idle" | "working">("idle");
  const [said, setSaid] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const account = accounts.find((one) => one.id === accountId) ?? accounts[0];
  if (!account) return null;

  const start = async () => {
    const ok = await confirm({
      title: `Download ${account.email} again?`,
      body:
        "Its mail on this computer is dropped and downloaded again from the server, newest first. Nothing changes on the server. " +
        "The account and its settings, drafts, the Outbox, pins, snoozes and reminders are kept. " +
        "A reply still waiting in the Outbox goes out, but the message it answers won't show as answered.",
      confirm: "Download again",
    });
    if (!ok) return;
    setState("working");
    setSaid(null);
    setError(null);
    resyncAccount(account.id)
      .then(
        (dropped) =>
          setSaid(
            `${dropped === 1 ? "1 message" : `${dropped.toLocaleString()} messages`} dropped from this computer; the newest are coming back first.`,
          ),
        (failure: unknown) => setError(String(failure)),
      )
      .finally(() => setState("idle"));
  };

  return (
    <>
      <div className="option">
        <span className="option-icon">
          <Icon name="refresh" size={18} />
        </span>
        <span className="option-text">
          <strong>Download an account again</strong>
          <span aria-live="polite">
            {state === "working"
              ? "Dropping its mail here… This can take up to half a minute."
              : (said ?? "When an account's mail looks wrong here. The server is not touched.")}
          </span>
          <span className="download-again">
            {accounts.length > 1 && (
              <Select
                label="Account to download again"
                value={account.id}
                options={accounts.map((one) => ({ id: one.id, label: one.email }))}
                onChange={setAccountId}
              />
            )}
            <button type="button" className="btn small" onClick={() => void start()} disabled={state === "working"}>
              Download again…
            </button>
          </span>
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
