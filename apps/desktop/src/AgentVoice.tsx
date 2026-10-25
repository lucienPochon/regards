import { useCallback, useEffect, useState } from "react";
import type { AccountSummary } from "./accounts";
import { useChanges } from "./changes";
import type { ConfirmRequest } from "./Confirm";
import { Icon } from "./icons";
import { type Identity, VOICE_MAX, learnVoicePrompt, listIdentities, setVoice } from "./identities";
import { type NoteSettings, forgetAllNotes, noteSettings, setNotesEnabled } from "./notes";
import { SubPageHead, Toggle } from "./SettingsControls";
import { t, tn } from "./i18n";

/** Every account's addresses, kept current. */
function useIdentities(accounts: AccountSummary[]): [Identity[], () => void] {
  const ids = accounts.map((account) => account.id).join(",");
  const [identities, setIdentities] = useState<Identity[]>([]);
  const refresh = useCallback(() => {
    const wanted = ids ? ids.split(",") : [];
    Promise.all(wanted.map((id) => listIdentities(id))).then(
      (lists) => setIdentities(lists.flat()),
      () => setIdentities([]),
    );
  }, [ids]);
  useEffect(refresh, [refresh]);
  useChanges(refresh);
  return [identities, refresh];
}

/**
 * The one-line request to hand Claude Code so it learns a voice from the
 * person's sent mail (ADR-0020 §3), to copy. What it suggests waits at the
 * top of Settings › Agents.
 */
function LearnIt({ email, onClose }: { email: string; onClose?: () => void }) {
  const [copied, setCopied] = useState(false);
  const prompt = learnVoicePrompt(email);
  const copy = () => {
    navigator.clipboard?.writeText(prompt).then(
      () => setCopied(true),
      () => setCopied(false),
    );
  };
  return (
    <div className="voice-ask">
      <div className="voice-ask-head">
        <strong>{t("Ask Claude Code")}</strong>
        {onClose && (
          <button type="button" className="icon-btn small" aria-label={t("Close")} title={t("Close")} onClick={onClose}>
            <Icon name="close" size={13} />
          </button>
        )}
      </div>
      <p className="voice-prompt">{prompt}</p>
      <div className="voice-ask-foot">
        <span className="muted" role="status">
          {copied
            ? t("Paste it in Claude Code. Its suggestion shows at the top of this page for you to approve.")
            : t("Claude Code reads your sent mail through Regards and suggests a voice. Nothing changes until you approve it.")}
        </span>
        <button type="button" className="btn small primary" onClick={copy}>
          {copied ? t("Copied") : t("Copy")}
        </button>
      </div>
    </div>
  );
}

/**
 * Settings › Agents › Your voice (ADR-0020): for each address, how agents
 * should write when they draft from it. On, a row opens the editor; empty,
 * ✦ hands Claude Code the request to learn it. Off, agents write plainly.
 */
export function VoiceSection({ accounts, onOpen }: { accounts: AccountSummary[]; onOpen: (identity: Identity) => void }) {
  const [identities, refresh] = useIdentities(accounts);
  const [asking, setAsking] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  if (identities.length === 0) return null;
  const toggle = (identity: Identity) => {
    setError(null);
    setVoice(identity.id, identity.voice, !identity.voice_on).then(refresh, (failure: unknown) => setError(String(failure)));
  };
  return (
    <section className="agent-section" aria-labelledby="voice-title">
      <div className="agent-section-head">
        <Icon name="quote" size={18} />
        <h4 id="voice-title">{t("Your voice")}</h4>
        <span
          className="info-tip"
          title={t("How agents should write when they draft for you from an address: language, tone, how formal, how you open and close, what never to promise. Only you can change it.")}
        >
          <Icon name="info" size={15} />
        </span>
      </div>
      <p className="muted agent-section-desc">
        {t("Agents read it before they draft for you from an address. Off: they write in a plain, neutral style.")}
      </p>
      {error && (
        <p className="error agent-section-desc" role="alert">
          {error}
        </p>
      )}
      <ul className="voice-list">
        {identities.map((identity) => {
          const empty = identity.voice.trim() === "";
          return (
            <li key={identity.id}>
              <div className="voice-row">
                {identity.voice_on ? (
                  <button type="button" className="voice-open" onClick={() => onOpen(identity)}>
                    <span className="voice-text">
                      <strong>{identity.email}</strong>
                      <span className={empty ? "muted" : "voice-preview"}>
                        {empty ? t("Empty. Write it, or ask Claude Code to learn it.") : identity.voice.split("\n")[0]}
                      </span>
                    </span>
                    <Icon name="chevronRight" size={15} />
                  </button>
                ) : (
                  <span className="voice-text">
                    <strong>{identity.email}</strong>
                    <span className="muted">{t("Off")}</span>
                  </span>
                )}
                {identity.voice_on && empty && (
                  <button
                    type="button"
                    className="icon-btn learn-btn"
                    title={t("Ask Claude Code to learn it from your sent mail")}
                    aria-label={t("Ask Claude Code to learn it")}
                    aria-expanded={asking === identity.id}
                    onClick={() => setAsking(asking === identity.id ? null : identity.id)}
                  >
                    <Icon name="agent" size={15} />
                  </button>
                )}
                <Toggle label={t("Use a voice for {address}", { address: identity.email })} on={identity.voice_on} onChange={() => toggle(identity)} />
              </div>
              {asking === identity.id && <LearnIt email={identity.email} onClose={() => setAsking(null)} />}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** One address's voice, written by the person (or learnt by an agent, then approved). */
export function VoiceEditor({ identity, onBack }: { identity: Identity; onBack: () => void }) {
  const [text, setText] = useState(identity.voice);
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = () => {
    const voice = text.trim();
    setBusy(true);
    setError(null);
    // Written, it is used; emptied, it stays as switched.
    setVoice(identity.id, voice, voice !== "" || identity.voice_on)
      .then(onBack, (failure: unknown) => setError(String(failure)))
      .finally(() => setBusy(false));
  };
  return (
    <>
      <SubPageHead title={t("Your voice")} onBack={onBack} />
      <div className="voice-editor">
        <div className="voice-editor-top">
          <span className="muted">{identity.email}</span>
          <button
            type="button"
            className="btn small"
            title={t("Ask Claude Code to learn it from your sent mail")}
            aria-expanded={asking}
            onClick={() => setAsking(!asking)}
          >
            <Icon name="agent" size={14} />
            {t("Learn it")}
          </button>
        </div>
        {asking && <LearnIt email={identity.email} />}
        <label htmlFor="voice-text" className="voice-label">
          {t("How you write from this address")}
        </label>
        <textarea
          id="voice-text"
          rows={9}
          maxLength={VOICE_MAX}
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder={t("For example: French by default, English if they write in English. Vous with clients. Short and warm. Never quote a price or promise a date.")}
        />
        <span className="muted small voice-count">
          {text.length} / {VOICE_MAX.toLocaleString()}
        </span>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="voice-buttons">
          <button type="button" className="btn small danger" onClick={() => setText("")} disabled={text === ""}>
            {t("Clear")}
          </button>
          <span className="spacer" />
          <button type="button" className="btn small" onClick={onBack}>
            {t("Cancel")}
          </button>
          <button type="button" className="btn small primary" onClick={save} disabled={busy}>
            {t("Save")}
          </button>
        </div>
      </div>
    </>
  );
}

/**
 * Settings › Agents › Notes about people (ADR-0020): one switch, how many
 * there are, and forgetting them all. Each note is seen and changed on the
 * person's card.
 */
export function NotesSection({ confirm }: { confirm: (request: ConfirmRequest) => Promise<boolean> }) {
  const [settings, setSettings] = useState<NoteSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(() => {
    noteSettings().then(setSettings, () => setSettings(null));
  }, []);
  useEffect(refresh, [refresh]);
  useChanges(refresh);
  if (!settings) return null;
  const switchTo = (on: boolean) => {
    setError(null);
    setNotesEnabled(on).then(setSettings, (failure: unknown) => setError(String(failure)));
  };
  const forget = async () => {
    const ok = await confirm({
      title: t("Forget every note?"),
      body: t("{notes} about {people} go, in every account. This can't be undone.", {
        notes: tn(settings.notes, "1 note", "{count} notes"),
        people: tn(settings.people, "1 person", "{count} people"),
      }),
      confirm: t("Forget them all"),
      danger: true,
    });
    if (!ok) return;
    forgetAllNotes().then(refresh, (failure: unknown) => setError(String(failure)));
  };
  return (
    <section className="agent-section" aria-labelledby="notes-title">
      <div className="agent-section-head">
        <Icon name="people" size={18} />
        <h4 id="notes-title">{t("Notes about people")}</h4>
        <Toggle label={t("Notes about people")} on={settings.on} onChange={switchTo} />
      </div>
      <p className="muted agent-section-desc">
        {t("Agents can read and add short notes about the people you write with: who decides, what they prefer. You see them on each person's card.")}
      </p>
      <p className="muted agent-section-fine">
        {settings.on ? (
          <>
            <span>
              {settings.notes === 0
                ? t("No notes yet")
                : t("{notes} about {people}", {
                    notes: tn(settings.notes, "1 note", "{count} notes"),
                    people: tn(settings.people, "1 person", "{count} people"),
                  })}
              {t(", kept on this computer.")}
            </span>
            {settings.notes > 0 && (
              <button type="button" className="danger-link" onClick={() => void forget()}>
                {t("Forget them all")}
              </button>
            )}
          </>
        ) : (
          t("Off: the notes are kept, but nobody is shown or given them, and agents add none.")
        )}
      </p>
      {error && (
        <p className="error agent-section-desc" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
