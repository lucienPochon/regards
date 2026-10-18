import { useCallback, useEffect, useState } from "react";
import { type DaemonStatus, fetchDaemonStatus } from "./daemon";

type Check =
  | { state: "checking" }
  | { state: "done"; status: DaemonStatus }
  | { state: "failed"; message: string };

export function App() {
  const [check, setCheck] = useState<Check>({ state: "checking" });

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
  if (status.error) {
    return <p className="error">{status.error}</p>;
  }
  if (!status.running) {
    return (
      <p>
        <span className="dot off" aria-hidden="true" /> Not running. Start it with <code>regards daemon run</code>.
      </p>
    );
  }
  return (
    <>
      <p>
        <span className="dot on" aria-hidden="true" /> Running · version {status.version} · up{" "}
        {status.uptimeSecs ?? 0}s
      </p>
      {status.sameBuild === false && (
        <p className="warning">
          The service is from a different build ({status.buildId}). Restart it to update.
        </p>
      )}
    </>
  );
}
