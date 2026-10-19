// Builds the `regards` program (the daemon, CLI and MCP server) for a
// release bundle. Tauri ships it next to the app as a sidecar, which is
// where the app looks for it (`regards_ipc::launcher::sibling_program`).
//
// Tauri wants it at `src-tauri/binaries/regards-<target triple>[.exe]`;
// it installs it as `regards[.exe]`. Run by `npm run release` (see
// `src-tauri/tauri.release.json`), for the target Tauri builds.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const host = /^host: (.+)$/m.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))[1].trim();
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || host;

const args = ["build", "--release", "--locked", "-p", "regards-cli"];
if (triple !== host) args.push("--target", triple);
execFileSync("cargo", args, { cwd: root, stdio: "inherit" });

const exe = triple.includes("windows") ? ".exe" : "";
const targetDir = process.env.CARGO_TARGET_DIR || join(root, "target");
const built = join(targetDir, ...(triple === host ? [] : [triple]), "release", `regards${exe}`);
const binaries = join(root, "apps", "desktop", "src-tauri", "binaries");
mkdirSync(binaries, { recursive: true });
copyFileSync(built, join(binaries, `regards-${triple}${exe}`));
console.log(`sidecar: regards-${triple}${exe}`);
