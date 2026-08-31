# Harness documentation website

This directory contains the public Docusaurus site for Harness. Its `docs/` tree is a deliberately
public-facing guide rather than a projection of the repository's internal designs and reviews.

## Develop

```bash
npm ci
npm run start
```

## Gate

```bash
npm run typecheck
npm run build
```

Broken links and anchors fail the production build. GitHub Pages builds every documentation pull
request and publishes the built site from `main`.

The website's build-only dependency review and exact security overrides are recorded in
[SECURITY.md](SECURITY.md). Run `npm audit` when changing the lock; an aggregate Docusaurus count is
not a substitute for checking the root advisory and its reach.
