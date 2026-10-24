// Refresh the copy of Thunderbird's mail provider database (the ISPDB) that
// Regards ships:
//
//   node crates/regards-ispdb/update.mjs <full commit sha>
//
// Choose a commit at least two weeks old (the project's rule for anything it
// ships). The repository at that commit is downloaded; its `ispdb/*.xml` and
// LICENSE are copied unchanged into `data/` and `LICENSE`, and `source.json`
// and `src/lib.rs` are written. Nothing else is touched.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repository = "thunderbird/autoconfig";
const commit = process.argv[2] ?? "";
if (!/^[0-9a-f]{40}$/.test(commit)) {
  console.error("usage: node crates/regards-ispdb/update.mjs <full commit sha>");
  process.exit(1);
}

const headers = { "User-Agent": "regards-dev-tooling" };
const info = await fetch(`https://api.github.com/repos/${repository}/commits/${commit}`, { headers });
if (!info.ok) throw new Error(`no such commit: ${info.status}`);
const date = (await info.json()).commit.committer.date;
const archive = await fetch(`https://codeload.github.com/${repository}/tar.gz/${commit}`, { headers });
if (!archive.ok) throw new Error(`download failed: ${archive.status}`);

const temp = mkdtempSync(join(tmpdir(), "ispdb-"));
try {
  writeFileSync(join(temp, "autoconfig.tar.gz"), Buffer.from(await archive.arrayBuffer()));
  // A relative name: GNU tar would read "C:\…" as a remote host.
  execFileSync("tar", ["-xzf", "autoconfig.tar.gz"], { cwd: temp });
  const top = join(temp, `autoconfig-${commit}`);

  const data = join(here, "data");
  rmSync(data, { recursive: true, force: true });
  mkdirSync(data);
  const files = readdirSync(join(top, "ispdb"))
    .filter((name) => name.endsWith(".xml") && /^[A-Za-z0-9._-]+$/.test(name))
    .sort();
  for (const name of files) copyFileSync(join(top, "ispdb", name), join(data, name));
  copyFileSync(join(top, "LICENSE"), join(here, "LICENSE"));

  writeFileSync(
    join(here, "source.json"),
    `${JSON.stringify({ repository, commit, date, files: files.length }, null, 2)}\n`,
  );
  const entries = files.map((name) => `    (${JSON.stringify(name)}, include_str!("../data/${name}")),`).join("\n");
  writeFileSync(
    join(here, "src", "lib.rs"),
    `//! Thunderbird's database of mail providers (the ISPDB), as Regards ships
//! it: each provider's settings file, unchanged, under the Mozilla Public
//! License 2.0 (\`LICENSE\`). Regards reads them on this computer
//! (\`regards_core::autoconfig\`); nothing is asked of Mozilla.
//!
//! Written by \`update.mjs\`; do not edit.

/// Where the files come from: the repository, its commit and that commit's date.
pub const SOURCE: (&str, &str, &str) = (${JSON.stringify(repository)}, ${JSON.stringify(commit)}, ${JSON.stringify(date.slice(0, 10))});

/// Each provider's settings file: its name, and what it says.
pub const FILES: &[(&str, &str)] = &[
${entries}
];
`,
  );
  console.log(`ispdb: ${files.length} files from ${repository}@${commit.slice(0, 12)} (${date.slice(0, 10)})`);
} finally {
  rmSync(temp, { recursive: true, force: true });
}
