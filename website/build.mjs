// Builds the website into website/dist: one page per language, from the same
// layout, styles and script. No dependencies: `node website/build.mjs`.
//
// - The version shown is the workspace's (Cargo.toml), so the page follows
//   each release without an edit.
// - Download buttons go to the latest GitHub release.
// - The languages must stay in step: the build fails when a page's elements
//   differ from the English page's, so a section added in one language is
//   not forgotten in the other.
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const here = import.meta.dirname;
const out = join(here, "dist");
const read = (...parts) => readFileSync(join(here, ...parts), "utf8");

const site = (process.env.SITE_URL || "https://lucienpochon.github.io/regards").replace(/\/$/, "");
const releases = "https://github.com/lucienPochon/regards/releases/latest";
const version = readFileSync(join(here, "..", "Cargo.toml"), "utf8")
  .match(/\[workspace\.package\][^[]*?^version = "([^"]+)"/ms)?.[1];
if (!version) throw new Error("no workspace version in Cargo.toml");

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
  const shared = { version, releases, root, site };
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
