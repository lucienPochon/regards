# The website

The public page that presents Regards, in English (`/`) and French (`/fr/`).
It is static: no framework, no dependencies, no requests to anyone but the
site itself (the fonts are in `assets/fonts`, under the SIL Open Font License).

```sh
node website/build.mjs      # writes website/dist
```

Open `website/dist/index.html` in a browser to see it.

- **`src/en.html`, `src/fr.html`:** the page in each language. They must have
  the same elements in the same order; the build fails, and says where, when
  they differ. Change both together.
- **`src/pages.json`:** each language's title and description.
- **`src/layout.html`:** the `<head>` both share.
- **`assets/`:** styles, script, fonts and the mark, copied as they are.

What the build fills in:

- `{{version}}`: the workspace version in `Cargo.toml`, so the page follows
  each release.
- `{{releases}}`: the latest GitHub release, where the download buttons go.

Say only what Regards does today, in step with `README.md` and
`docs/agents.md`.

## Publishing

`.github/workflows/website.yml` builds the site on every change to it. It
publishes to GitHub Pages from `main` once the repository has Pages set to
**GitHub Actions** (Settings › Pages) and the variable `WEBSITE_DEPLOY` set to
`true`. Set the variable `WEBSITE_URL` to the site's address when it has its
own domain; it is used for the canonical and language links.
