// Every icon of the app, from the drawings in src-tauri/icons/source/
// (`node scripts/icons.mjs` after changing one). The Tauri CLI draws each PNG
// from its SVG; this script packs the Windows .ico and the macOS .icns itself,
// so that their small sizes use the small cut, drawn heavier for 48 px and less.
//
// - mark.svg, mark-small.svg: the mark, full bleed (Windows, Linux).
// - mark-macos.svg, mark-macos-small.svg: on Apple's icon grid, 824 of 1024,
//   with its shadow (macOS draws neither).
// - tray-template.svg: one colour for the macOS menu bar, which tints it.

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const desktop = join(dirname(fileURLToPath(import.meta.url)), "..");
const source = join(desktop, "src-tauri", "icons", "source");
const icons = join(desktop, "src-tauri", "icons");
const scratch = mkdtempSync(join(tmpdir(), "regards-icons-"));

/** The SVG drawn at each size: size → PNG bytes. */
function draw(svg, sizes) {
  const out = join(scratch, svg);
  const cli = join(desktop, "node_modules", "@tauri-apps", "cli", "tauri.js");
  execFileSync(process.execPath, [cli, "icon", join(source, svg), "-o", out, "-p", sizes.join(",")], {
    cwd: desktop,
    stdio: "ignore",
  });
  return Object.fromEntries(sizes.map((size) => [size, readFileSync(join(out, `${size}x${size}.png`))]));
}

/** A Windows icon file holding these PNGs (Vista and later read PNG entries). */
function ico(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(images.length, 4);
  const directory = Buffer.alloc(16 * images.length);
  let offset = header.length + directory.length;
  images.forEach(([size, png], index) => {
    const at = index * 16;
    directory.writeUInt8(size >= 256 ? 0 : size, at);
    directory.writeUInt8(size >= 256 ? 0 : size, at + 1);
    directory.writeUInt16LE(1, at + 4);
    directory.writeUInt16LE(32, at + 6);
    directory.writeUInt32LE(png.length, at + 8);
    directory.writeUInt32LE(offset, at + 12);
    offset += png.length;
  });
  return Buffer.concat([header, directory, ...images.map(([, png]) => png)]);
}

/** A macOS icon file: each entry a four-letter type and its PNG. */
function icns(entries) {
  const chunks = entries.map(([type, png]) => {
    const head = Buffer.alloc(8);
    head.write(type, 0, "ascii");
    head.writeUInt32BE(png.length + 8, 4);
    return Buffer.concat([head, png]);
  });
  const body = Buffer.concat(chunks);
  const head = Buffer.alloc(8);
  head.write("icns", 0, "ascii");
  head.writeUInt32BE(body.length + 8, 4);
  return Buffer.concat([head, body]);
}

const small = draw("mark-small.svg", [16, 24, 30, 32, 44, 48]);
const regular = draw("mark.svg", [50, 64, 71, 89, 107, 128, 142, 150, 256, 284, 310, 512]);
const macSmall = draw("mark-macos-small.svg", [16, 32]);
const mac = draw("mark-macos.svg", [64, 128, 256, 512, 1024]);
const template = draw("tray-template.svg", [32]);

const files = {
  "32x32.png": small[32],
  "64x64.png": regular[64],
  "128x128.png": regular[128],
  "128x128@2x.png": regular[256],
  "icon.png": regular[512],
  "Square30x30Logo.png": small[30],
  "Square44x44Logo.png": small[44],
  "Square71x71Logo.png": regular[71],
  "Square89x89Logo.png": regular[89],
  "Square107x107Logo.png": regular[107],
  "Square142x142Logo.png": regular[142],
  "Square150x150Logo.png": regular[150],
  "Square284x284Logo.png": regular[284],
  "Square310x310Logo.png": regular[310],
  "StoreLogo.png": regular[50],
  "tray.png": small[32],
  "tray-template.png": template[32],
  "icon.ico": ico([
    [16, small[16]],
    [24, small[24]],
    [32, small[32]],
    [48, small[48]],
    [64, regular[64]],
    [128, regular[128]],
    [256, regular[256]],
  ]),
  "icon.icns": icns([
    ["icp4", macSmall[16]],
    ["ic11", macSmall[32]],
    ["icp5", macSmall[32]],
    ["ic12", mac[64]],
    ["icp6", mac[64]],
    ["ic07", mac[128]],
    ["ic13", mac[256]],
    ["ic08", mac[256]],
    ["ic14", mac[512]],
    ["ic09", mac[512]],
    ["ic10", mac[1024]],
  ]),
};
for (const [name, bytes] of Object.entries(files)) writeFileSync(join(icons, name), bytes);
rmSync(scratch, { recursive: true, force: true });
console.log(`icons: ${Object.keys(files).length} files in src-tauri/icons`);
