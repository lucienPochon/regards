import { useEffect, useState } from "react";
import { type AgentsSettings, agentsSettings, disconnectAgents, setAgentsEnabled } from "./agents";
import { Option, Toggle } from "./SettingsControls";

/**
 * Settings › Agents, at the top: one switch for every agent at once, and the
 * agents connected now, to disconnect. Off, each account keeps its level for
 * when they are on again.
 */
export function AgentsGate() {
  const [gate, setGate] = useState<AgentsSettings | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    agentsSettings().then(setGate, (failure: unknown) => setError(String(failure)));
  }, []);

  const turn = (on: boolean) => {
    setError(null);
    setSaid(null);
    const before = gate?.connected ?? 0;
    setAgentsEnabled(on).then(
      (now) => {
        setGate(now);
        if (!on && before > 0) setSaid(`${before === 1 ? "The agent" : `The ${before} agents`} connected now ${before === 1 ? "was" : "were"} disconnected.`);
      },
      (failure: unknown) => setError(String(failure)),
    );
  };
  const disconnect = () => {
    setError(null);
    disconnectAgents().then(
      (count) => {
        setGate((now) => (now ? { ...now, connected: 0 } : now));
        setSaid(
          count === 0
            ? "No agent was connected."
            : `${count === 1 ? "1 agent" : `${count} agents`} disconnected. They can connect again.`,
        );
      },
      (failure: unknown) => setError(String(failure)),
    );
  };

  if (!gate) return error ? <p className="error" role="alert">{error}</p> : null;
  return (
    <div className="agents-gate">
      <Option
        icon="agent"
        label="Agents can use Regards"
        description={
          gate.enabled
            ? "Each account below decides what they may do."
            : "Off: every agent request is refused. Each account keeps its level for when you turn this on again."
        }
      >
        <Toggle label="Agents can use Regards" on={gate.enabled} onChange={turn} />
      </Option>
      {gate.enabled && (
        <div className="option">
          <span className="option-icon" aria-hidden="true" />
          <span className="option-text">
            <strong>
              {gate.connected === 0
                ? "No agent connected now"
                : `${gate.connected === 1 ? "1 agent" : `${gate.connected} agents`} connected now`}
            </strong>
            <span>Disconnecting ends their connection; they can connect again.</span>
          </span>
          <span className="option-control">
            <button type="button" className="btn small" onClick={disconnect} disabled={gate.connected === 0}>
              Disconnect now
            </button>
          </span>
        </div>
      )}
      <p className="muted small" role="status">
        {said}
      </p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
