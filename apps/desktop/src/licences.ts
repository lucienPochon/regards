/**
 * Third-party licences (Support › Licences). The Rust side's list is made
 * at release time by `scripts/licences.mjs` (cargo-about) into
 * `public/licences/rust.json`, and ships with the app's files.
 */

/** A licence's text, kept once however many crates use it. */
export interface LicenceText {
  /** SPDX id: "MIT", "Apache-2.0" … */
  id: string;
  name: string;
  text: string;
}

export interface LicensedCrate {
  name: string;
  version: string;
  /** The crate's SPDX expression: "MIT OR Apache-2.0". */
  licence: string;
  /** Indices into `texts`: the texts that apply. */
  texts: number[];
}

export interface LicenceList {
  generated: boolean;
  /** "cargo-about 0.9.2". */
  tool: string | null;
  texts: LicenceText[];
  crates: LicensedCrate[];
}

/** The Rust programs' licences; `null` when this build has none (a development build without cargo-about). */
export async function rustLicences(): Promise<LicenceList | null> {
  try {
    const response = await fetch("/licences/rust.json");
    if (!response.ok) return null;
    const list = (await response.json()) as LicenceList;
    return list.generated ? list : null;
  } catch {
    return null;
  }
}
