import { useCallback, useEffect, useState } from "react";
import { AccountsPanel } from "./AccountsPanel";
import { MessageList, type OpenMailbox } from "./MessageList";
import { type DaemonStatus, fetchDaemonStatus } from "./daemon";

type Check =
  | { state: "checking" }
  | { state: "done"; status: DaemonStatus }
  | { state: "failed"; message: string };

export function App() {
  const [check, setCheck] = useState<Check>({ state: "checking" });
  const [open, setOpen] = useState<OpenMailbox | null>(null);

  const refresh = useCallback(() => {
    setCheck({ state: "checking" });
    fetchDaemonStatus()
      .then((status) => setCheck({ state: "done", status }))
      .catch((error: unknown) => setCheck({ state: "failed", message: String(error) }));
  }, []);

  useEffect(() => {
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh]);

  return (
    <main className="shell">
      <header>
        <h1>Regards</h1>
        <p className="subtitle">Pre-alpha · Phase 0 foundation</p>
      </header>

      <section className="card" aria-live="polite">
        <h2>Background service</h2>
        <ServiceStatus check={check} />
        <button type="button" onClick={refresh} disabled={check.state === "checking"}>
          Check again
        </button>
      </section>

      <AccountsPanel selected={open?.id ?? null} onOpen={setOpen} />
      {open && <MessageList mailbox={open} />}
    </main>
  );
}

function ServiceStatus({ check }: { check: Check }) {
  if (check.state === "checking") {
    return <p className="muted">Checking…</p>;
  }
  if (check.state === "failed") {
    return <p className="error">Could not check the service: {check.message}</p>;
  }
  const { status } = check;
  if (status.error || !status.running) {
    return (
      <p className="error">
        <span className="dot off" aria-hidden="true" /> The service could not be started.{" "}
        {status.error}
      </p>
    );
  }
  return (
    <>
      <p>
        <span className="dot on" aria-hidden="true" /> Running · version {status.version} · pid{" "}
        {status.pid} · up {status.uptimeSecs ?? 0}s
      </p>
      <p className="muted">
        {status.desktopAccess ? "This app has desktop access." : "This app has agent access only."}
        {status.started && " Started by this app just now."}
        {status.replaced && " An older or separately started service was replaced."}
      </p>
    </>
  );
}
