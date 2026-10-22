// The third-party licences of the web packages the window ships (Support ›
// Licences): the production dependencies, as installed. Writes
// `public/licences/npm.json` in the same shape as `scripts/licences.mjs`
// writes the Rust side's:
//
//   { generated, tool, texts: [{ id, name, text }],
//     crates: [{ name, version, licence, texts: [index into texts] }] }
//
// (`crates` keeps the Rust side's name: here they are npm packages.) Each
// licence text is kept once. Development packages (the build tools, tests)
// are not shipped, so they are not listed.

import { execSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const app = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(app, "public", "licences", "npm.json");

/** Names for the SPDX ids these packages use; the id itself otherwise. */
const NAMES = {
  MIT: "MIT License",
  "Apache-2.0": "Apache License 2.0",
  ISC: "ISC License",
  "0BSD": "BSD Zero Clause License",
  "BSD-2-Clause": "BSD 2-Clause License",
  "BSD-3-Clause": "BSD 3-Clause License",
  "MPL-2.0": "Mozilla Public License 2.0",
  Unlicense: "The Unlicense",
};

// Every production package, as a folder: `npm ls` resolves nested and hoisted copies alike.
const folders = execSync("npm ls --omit=dev --all --parseable", { cwd: app, encoding: "utf8" })
  .split(/\r?\n/)
  .map((line) => line.trim())
  .filter((line) => line && line !== app);

const licenceOf = (pkg) => {
  const field = pkg.license ?? pkg.licenses;
  if (typeof field === "string") return field;
  if (Array.isArray(field)) return field.map((one) => (typeof one === "string" ? one : one.type)).join(" OR ");
  if (field && typeof field === "object") return field.type ?? "UNKNOWN";
  return "UNKNOWN";
};

// A licence file: LICENSE, LICENCE.md, COPYING, or one per licence of a choice
// (LICENSE-MIT, LICENSE_APACHE-2.0). Not LICENSE.spdx, which only names it.
const LICENCE_FILE = /^(licen[cs]e|copying)([._-][\w.-]*)?$/i;

/** A package's licence texts, each with the licence it is. */
const textsOf = (folder, licence) =>
  readdirSync(folder)
    .filter((name) => LICENCE_FILE.test(name) && !/\.spdx$/i.test(name))
    .map((name) => {
      const id = /apache/i.test(name) ? "Apache-2.0" : /mit/i.test(name) ? "MIT" : licence;
      return { id, text: readFileSync(join(folder, name), "utf8").trim() };
    })
    .filter(({ text }) => text.length > 0);

const texts = [];
const known = new Map(); // id + text -> index into texts
const crates = [];
const seen = new Set();
for (const folder of folders) {
  const manifest = join(folder, "package.json");
  if (!existsSync(manifest)) continue;
  const pkg = JSON.parse(readFileSync(manifest, "utf8"));
  const key = `${pkg.name}@${pkg.version}`;
  if (seen.has(key)) continue;
  seen.add(key);
  const licence = licenceOf(pkg).replace(/^\((.*)\)$/, "$1");
  const indices = [];
  for (const { id, text } of textsOf(folder, licence)) {
    const textKey = `${id}\n${text.replace(/\s+/g, " ")}`;
    if (!known.has(textKey)) known.set(textKey, texts.push({ id, name: NAMES[id] ?? id, text }) - 1);
    indices.push(known.get(textKey));
  }
  crates.push({ name: pkg.name, version: pkg.version, licence, texts: indices });
}
crates.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify({ generated: true, tool: "scripts/licences-npm.mjs", texts, crates }));
console.log(`licences: ${crates.length} npm packages, ${texts.length} licence texts`);
