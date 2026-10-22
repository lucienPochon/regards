import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  AGENT_APPS,
  type AgentApp,
  type AgentsSettings,
  agentAppsFound,
  agentsSettings,
  type FoundAgentApp,
  isConnected,
  isFound,
  openInAgent,
} from "./agents";
import { openUrl } from "./background";
import { useDialogFocus } from "./dialogFocus";
import { Icon } from "./icons";
import type { Anchor } from "./Menu";
import { setPrefs } from "./prefs";
import { type Recipe, recipeGroups } from "./recipes";
import { useShortcuts } from "./shortcuts";

/** The agent guide, on the project's site. */
const GUIDE = "https://github.com/lucienPochon/regards/blob/main/docs/agents.md";
const MARGIN = 8;

/** A small mark for each agent app: its initials, not its logo. */
function AgentMark({ app }: { app: AgentApp }) {
  const initials = { "claude-code": "CC", "claude-app": "C", codex: "Cx", cursor: "Cu" }[app.id];
  return (
    <span className={`agent-mark ${app.id}`} aria-hidden="true">
      {initials}
    </span>
  );
}

/**
 * The sidebar's Agents menu: recipes (ready-made requests, handed to an
 * agent with a click or copied) and the agent apps on this computer, the
 * connected ones marked active. Regards itself sends nothing to any AI: the
 * agent gets the request ready, and the person sends it there.
 */
export function AgentsMenu({
  anchor,
  onSettings,
  onClose,
}: {
  anchor: Anchor;
  /** Settings › Agents: connecting an agent, and what each account allows. */
  onSettings: () => void;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<"recipes" | "agents">("recipes");
  const [recipe, setRecipe] = useState<Recipe | null>(null);
  const [settings, setSettings] = useState<AgentsSettings | null>(null);
  // Undefined while looking; null when looking failed.
  const [found, setFound] = useState<FoundAgentApp[] | null | undefined>(undefined);
  const [said, setSaid] = useState<string | null>(null);
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);
  const card = useRef<HTMLDivElement>(null);
  useDialogFocus(card);
  useShortcuts({ Escape: onClose }, { modal: true });

  useEffect(() => {
    agentsSettings().then(setSettings, () => setSettings(null));
    agentAppsFound().then(setFound, () => setFound(null));
  }, []);

  // The apps found on this computer; all of them, to connect one, when none is.
  const here = found ? AGENT_APPS.filter((app) => isFound(app, settings, found)) : [];
  const shown = here.length > 0 ? here : AGENT_APPS;

  // Beside the button it opened from, its foot level with the button's.
  useLayoutEffect(() => {
    const box = card.current?.getBoundingClientRect();
    if (!box) return;
    const left = "rect" in anchor ? anchor.rect.right + 16 : anchor.x;
    const bottom = "rect" in anchor ? anchor.rect.bottom : anchor.y;
    setPosition({
      left: Math.max(MARGIN, Math.min(left, window.innerWidth - box.width - MARGIN)),
      top: Math.max(MARGIN, Math.min(bottom - box.height, window.innerHeight - box.height - MARGIN)),
    });
  }, [anchor, tab, recipe, said]);

  const toSettings = () => {
    onClose();
    onSettings();
  };
  const choose = (next: Recipe | null) => {
    setRecipe(next);
    setSaid(null);
  };
  const send = (app: AgentApp) => {
    if (!recipe || (app.id !== "claude-code" && app.id !== "claude-app")) return;
    openInAgent(app.id, recipe.prompt).then(
      () => setSaid(app.hint),
      (failure: unknown) => setSaid(`${String(failure)}. Copy the request instead, and paste it in ${app.label}.`),
    );
  };
  const copy = () => {
    if (!recipe) return;
    navigator.clipboard?.writeText(recipe.prompt).then(
      () => setSaid("Copied: paste it in your agent."),
      () => setSaid("Copying didn't work here."),
    );
  };

  return (
    <div className="menu-layer" onMouseDown={onClose}>
      <div
        ref={card}
        className={position ? "agents-menu" : "agents-menu measuring"}
        role="dialog"
        aria-label="Agents"
        tabIndex={-1}
        style={position ?? undefined}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="agents-tabs" role="tablist" aria-label="Agents">
          {(["recipes", "agents"] as const).map((id) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={tab === id}
              className={tab === id ? "agents-tab chosen" : "agents-tab"}
              onClick={() => setTab(id)}
            >
              {id === "recipes" ? "Recipes" : "Agents"}
            </button>
          ))}
        </div>

        <div className="agents-body" role="tabpanel">
          {tab === "recipes" && !recipe && (
            <>
              <p className="agents-lead">Ready-made requests for an AI agent connected to Regards.</p>
              {recipeGroups().map(({ group, recipes }) => (
                <section key={group} aria-label={group}>
                  <h4 className="agents-group">{group}</h4>
                  <ul className="agents-list">
                    {recipes.map((entry) => (
                      <li key={entry.id}>
                        <button type="button" className="agents-row" onClick={() => choose(entry)}>
                          <Icon name={entry.icon} size={16} />
                          <span className="agents-row-label">{entry.title}</span>
                          <Icon name="chevronRight" size={14} className="agents-chevron" />
                        </button>
                      </li>
                    ))}
                  </ul>
                </section>
              ))}
            </>
          )}

          {tab === "recipes" && recipe && (
            <>
              <div className="agents-back">
                <button type="button" className="icon-btn small" aria-label="Back to the recipes" onClick={() => choose(null)}>
                  <Icon name="back" size={16} />
                </button>
                <strong>{recipe.title}</strong>
              </div>
              <p className="agents-lead">{recipe.caption}</p>
              <h4 className="agents-group">Send to an AI agent</h4>
              <ul className="agents-list">
                {AGENT_APPS.filter((app) => app.opens).map((app) => (
                  <li key={app.id}>
                    <button type="button" className="agents-row" onClick={() => send(app)}>
                      <AgentMark app={app} />
                      <span className="agents-row-label">{app.label}</span>
                      {isConnected(app, settings, found ?? null) && <span className="agents-active">Active</span>}
                    </button>
                  </li>
                ))}
                <li className="agents-split">
                  <button type="button" className="agents-row" onClick={copy}>
                    <Icon name="copy" size={16} />
                    <span className="agents-row-label">Copy to the clipboard</span>
                  </button>
                </li>
              </ul>
              {said && (
                <p className="agents-said" role="status">
                  {said}
                </p>
              )}
            </>
          )}

          {tab === "agents" && (
            <>
              <h4 className="agents-group first">AI agents on this computer</h4>
              <p className="agents-lead">
                {found === undefined
                  ? "Looking for agents on this computer…"
                  : here.length > 0
                    ? "Regards found these agents on this computer. A connected one reads and acts on your mail through Regards, within what each account allows."
                    : found
                      ? "Regards didn't find an AI agent on this computer. Connect one to read and act on your mail through Regards, within what each account allows."
                      : "A connected agent reads and acts on your mail through Regards, within what each account allows."}
              </p>
              <ul className="agents-list">
                {(found === undefined ? [] : shown).map((app) => (
                  <li key={app.id} className="agents-app">
                    <AgentMark app={app} />
                    <span className="agents-row-label">{app.label}</span>
                    {isConnected(app, settings, found ?? null) ? (
                      <span className="agents-active">Active</span>
                    ) : (
                      <button type="button" className="pill-btn" onClick={toSettings}>
                        Connect
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>

        <div className="agents-foot">
          <button type="button" className="agents-learn" onClick={() => void openUrl(GUIDE)}>
            Learn more
            <Icon name="external" size={13} />
          </button>
          <button
            type="button"
            className="icon-btn small"
            title="Hide this button (Settings › Agents shows it again)"
            aria-label="Hide the Agents button"
            onClick={() => {
              setPrefs({ agentsButton: false });
              onClose();
            }}
          >
            <Icon name="eyeOff" size={16} />
          </button>
          <button type="button" className="icon-btn small" title="Agent settings" aria-label="Agent settings" onClick={toSettings}>
            <Icon name="settings" size={16} />
          </button>
        </div>
      </div>
    </div>
  );
}
