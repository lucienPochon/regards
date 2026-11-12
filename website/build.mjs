// Builds the website into website/dist: one page per language, from the same
// layout, styles and script. No dependencies: `node website/build.mjs`.
//
// - The version shown is the workspace's (Cargo.toml), so the page follows
//   each release without an edit.
// - Download buttons fetch the installer itself, from the GitHub release of
//   that version. Until that release is published (it starts as a draft),
//   they go to the releases page instead: the build asks GitHub for each file.
// - The languages must stay in step: the build fails when a page's elements
//   differ from the English page's, so a section added in one language is
//   not forgotten in the other.
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const here = import.meta.dirname;
const out = join(here, "dist");
const read = (...parts) => readFileSync(join(here, ...parts), "utf8");

// The site's address: SITE_URL when given, else the Vercel project's
// production domain (set by Vercel during its builds), else the default.
const DEFAULT_SITE = "https://regards-two.vercel.app";
const onVercel = process.env.VERCEL_PROJECT_PRODUCTION_URL;
const site = (process.env.SITE_URL || (onVercel ? `https://${onVercel}` : DEFAULT_SITE)).replace(/\/$/, "");
const releases = "https://github.com/lucienPochon/regards/releases/latest";
const version = readFileSync(join(here, "..", "Cargo.toml"), "utf8")
  .match(/\[workspace\.package\][^[]*?^version = "([^"]+)"/ms)?.[1];
if (!version) throw new Error("no workspace version in Cargo.toml");

// The installers of this version, as the release workflow names them.
const files = {
  dl_windows: `Regards_${version}_x64-setup.exe`,
  dl_mac_arm: `Regards_${version}_aarch64.dmg`,
  dl_mac_intel: `Regards_${version}_x64.dmg`,
  dl_deb: `Regards_${version}_amd64.deb`,
  dl_rpm: `Regards-${version}-1.x86_64.rpm`,
  dl_appimage: `Regards_${version}_amd64.AppImage`,
};
/** The file's own address when GitHub has it (it answers with a redirect to the file), else the releases page. */
async function download(name) {
  const url = `https://github.com/lucienPochon/regards/releases/download/v${version}/${name}`;
  try {
    const answer = await fetch(url, { method: "HEAD", redirect: "manual", signal: AbortSignal.timeout(8000) });
    if (answer.status === 302 || answer.ok) return url;
    console.warn(`${name}: not published (${answer.status}), linking to the releases page`);
  } catch (error) {
    console.warn(`${name}: could not ask GitHub (${error.message}), linking to the releases page`);
  }
  return releases;
}
const downloads = Object.fromEntries(
  await Promise.all(Object.entries(files).map(async ([key, name]) => [key, await download(name)])),
);

/** A page's elements, without its words: tag names, ids and classes, in order. */
function shape(html) {
  return [...html.matchAll(/<(\/?)([a-z][a-z0-9]*)([^>]*)>/g)].map(([, close, tag, rest]) => {
    const id = rest.match(/\bid="([^"]*)"/)?.[1] ?? "";
    const cls = rest.match(/\bclass="([^"]*)"/)?.[1] ?? "";
    return `${close}${tag}#${id}.${cls}`;
  });
}

const escape = (text) => text.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");
const fill = (text, values) =>
  text.replace(/\{\{(\w+)\}\}/g, (_, key) => {
    if (!(key in values)) throw new Error(`nothing to put in {{${key}}}`);
    return values[key];
  });

const layout = read("src", "layout.html");
const pages = JSON.parse(read("src", "pages.json"));
const english = shape(read("src", "en.html"));

rmSync(out, { recursive: true, force: true });
for (const [lang, page] of Object.entries(pages)) {
  const body = read("src", `${lang}.html`);
  const elements = shape(body);
  const at = elements.findIndex((element, i) => element !== english[i]);
  if (at !== -1 || elements.length !== english.length) {
    const where = at === -1 ? Math.min(elements.length, english.length) : at;
    throw new Error(
      `${lang}.html is out of step with en.html at element ${where}: ` +
        `${elements[where] ?? "(nothing)"} where English has ${english[where] ?? "(nothing)"}`,
    );
  }
  const root = page.path ? "../".repeat(page.path.split("/").filter(Boolean).length) : "";
  const shared = { version, releases, ...downloads, root, site, shots: page.shots ?? "" };
  const html = fill(layout, {
    ...shared,
    lang,
    locale: page.locale,
    url: `${site}/${page.path}`,
    title: escape(page.title),
    description: escape(page.description),
    other_lang: page.other_lang,
    other_href: page.other_href,
    other_text: escape(page.other_text),
    body: fill(body, shared),
  });
  mkdirSync(join(out, page.path), { recursive: true });
  writeFileSync(join(out, page.path, "index.html"), html);
  console.log(`${lang}: dist/${page.path}index.html`);
}
cpSync(join(here, "assets"), join(out, "assets"), { recursive: true });
// GitHub Pages: serve the files as they are.
writeFileSync(join(out, ".nojekyll"), "");
console.log(`Regards ${version}, for ${site}`);
