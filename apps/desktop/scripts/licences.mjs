// The third-party licences of the Rust programs Regards ships (Support ›
// Licences), from cargo-about and the repository's about.toml. Writes
// `public/licences/rust.json`, which the app reads (see `src/licences.ts`):
//
//   { generated, tool, texts: [{ id, name, text }],
//     crates: [{ name, version, licence, texts: [index into texts] }] }
//
// Each licence text is kept once: hundreds of crates share a few.
//
// `--required` (release builds) fails without cargo-about. Otherwise a
// missing cargo-about writes an empty list, so development builds work.

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const out = join(root, "apps", "desktop", "public", "licences", "rust.json");
const required = process.argv.includes("--required");

function write(list) {
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, JSON.stringify(list));
}

let about;
try {
  const tool = execFileSync("cargo", ["about", "--version"], { cwd: root, encoding: "utf8" }).trim();
  const raw = execFileSync("cargo", ["about", "generate", "--format", "json"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    stdio: ["ignore", "pipe", "inherit"],
  });
  about = { tool, data: JSON.parse(raw) };
} catch (error) {
  if (required) {
    console.error("licences: cargo-about is needed for a release (cargo install cargo-about --version =0.9.2 --locked --features cli)");
    throw error;
  }
  console.warn("licences: cargo-about is not installed; the licence list stays empty in this build");
  write({ generated: false, tool: null, texts: [], crates: [] });
  process.exit(0);
}

const texts = [];
const used = new Map(); // "name@version" -> indices into texts
for (const licence of about.data.licenses) {
  const index = texts.push({ id: licence.id, name: licence.name, text: licence.text }) - 1;
  for (const { crate } of licence.used_by) {
    const key = `${crate.name}@${crate.version}`;
    if (!used.has(key)) used.set(key, []);
    used.get(key).push(index);
  }
}
const crates = about.data.crates
  .map(({ package: pkg, license }) => ({
    name: pkg.name,
    version: pkg.version,
    licence: license,
    texts: used.get(`${pkg.name}@${pkg.version}`) ?? [],
  }))
  .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

write({ generated: true, tool: about.tool, texts, crates });
console.log(`licences: ${crates.length} crates, ${texts.length} licence texts (${about.tool})`);
