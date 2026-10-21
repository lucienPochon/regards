import { useEffect, useMemo, useState } from "react";
import { type LicenceList, rustLicences } from "./licences";
import { SubPageHead } from "./SettingsControls";

/** The web packages' list, made at release time by scripts/licences-npm.mjs, in the Rust side's shape. */
async function webLicences(): Promise<LicenceList | null> {
  try {
    const response = await fetch("/licences/npm.json");
    if (!response.ok) return null;
    const list = (await response.json()) as LicenceList;
    return list.generated ? list : null;
  } catch {
    return null;
  }
}

/** One list, by licence text: which components each text covers, the text behind a disclosure. */
function LicenceGroups({ title, list, filter }: { title: string; list: LicenceList | null | undefined; filter: string }) {
  const groups = useMemo(() => {
    if (!list) return [];
    const words = filter.trim().toLowerCase();
    const shown = list.crates.filter((component) => !words || component.name.toLowerCase().includes(words));
    const byText = list.texts.map((text, index) => ({
      text,
      components: shown.filter((component) => component.texts.includes(index)),
    }));
    const withoutText = shown.filter((component) => component.texts.length === 0);
    return [
      ...byText.filter((group) => group.components.length > 0),
      ...(withoutText.length > 0 ? [{ text: null, components: withoutText }] : []),
    ];
  }, [list, filter]);

  return (
    <section className="licence-list" aria-label={title}>
      <h4 className="settings-subhead">{title}</h4>
      {list === undefined ? (
        <p className="muted">Reading the list…</p>
      ) : list === null ? (
        <p className="muted">Not in this build: the list is made when Regards is built for release.</p>
      ) : (
        <>
          <p className="muted small">
            {list.crates.length.toLocaleString()} components, {list.texts.length.toLocaleString()} licence texts
            {list.tool ? ` (from ${list.tool})` : ""}.
          </p>
          {groups.length === 0 && <p className="muted">Nothing here matches.</p>}
          {groups.map((group, index) => (
            <details key={index} className="licence-group">
              <summary>
                <strong>{group.text ? group.text.name : "Licence named, no text shipped"}</strong>
                <span className="muted">
                  {" "}
                  · {group.components.length === 1 ? "1 component" : `${group.components.length} components`}
                </span>
                <span className="licence-names">
                  {group.components.map((component) => `${component.name} ${component.version}`).join(", ")}
                </span>
              </summary>
              {group.text ? (
                <pre className="licence-text">{group.text.text}</pre>
              ) : (
                <ul className="licence-bare">
                  {group.components.map((component) => (
                    <li key={`${component.name}@${component.version}`}>
                      {component.name} {component.version}: {component.licence}
                    </li>
                  ))}
                </ul>
              )}
            </details>
          ))}
        </>
      )}
    </section>
  );
}

/** Support › Open-source licences: Regards' own, then those of what it is built on. */
export function LicencesPage({ onBack }: { onBack: () => void }) {
  const [rust, setRust] = useState<LicenceList | null | undefined>(undefined);
  const [web, setWeb] = useState<LicenceList | null | undefined>(undefined);
  const [filter, setFilter] = useState("");
  useEffect(() => {
    rustLicences().then(setRust);
    webLicences().then(setWeb);
  }, []);
  return (
    <>
      <SubPageHead title="Open-source licences" onBack={onBack} />
      <p className="muted settings-note">
        Regards is open source, under the Apache License 2.0 or the MIT License, as you choose. It is built on the work of
        the projects below; their licences follow.
      </p>
      <input
        className="input licence-filter"
        type="search"
        placeholder="Find a component"
        aria-label="Find a component"
        value={filter}
        onChange={(event) => setFilter(event.target.value)}
      />
      <LicenceGroups title="The service and the command line (Rust)" list={rust} filter={filter} />
      <LicenceGroups title="The window (web packages)" list={web} filter={filter} />
    </>
  );
}
