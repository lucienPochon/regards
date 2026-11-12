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

## Keep it true

The page says only what Regards does today, in step with `README.md` and
`docs/agents.md`. A pull request that changes what the page describes (a
feature, what agents can do, providers, platforms, installers, signing,
updates, the price) changes the page too, in both languages. At each release
the page is read again (`docs/releasing.md`).

## Publishing

The site is at <https://regards-two.vercel.app>, on Vercel (project `regards`).
`vercel.json` at the root of the repository says how it is built: no install,
`node website/build.mjs`, and `website/dist` served. `.vercelignore` keeps
everything but `website/` and `Cargo.toml` out of what is uploaded.

- **By hand:** `vercel deploy --prod` from the root of the repository, signed
  in to the project's Vercel account.
- **By itself:** once the repository is connected to the project
  (`vercel git connect`, or the project's Git settings on Vercel), each merge
  to `main` that touches `website/`, `Cargo.toml` or `vercel.json` is
  published, and each pull request gets a preview.

The canonical and language links use the project's production domain. With a
domain of its own, add it to the Vercel project and set `SITE_URL` there.

`.github/workflows/website.yml` only checks that the site builds.
