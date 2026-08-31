# Website build dependency review

Reviewed: 2026-08-31.

The website is compiled to static files. Its Node dependency graph is build tooling and is not
served by the resulting site. `package.json` overrides `serialize-javascript` to `7.1.1` and `uuid`
to `11.1.1`; those are the compatible fixed releases for the advisories found in the 2026-08-31
review.

The published `image-size@2.0.2` release used by Docusaurus has no fixed successor and carries two
infinite-loop advisories:

- `GHSA-w3rx-r6r6-pgpr`: an infinite loop in the ICNS parser.
- `GHSA-5p2g-fcmc-qvqq`: infinite loops in the JXL and HEIF parsers.

The lock replaces that transitive package with the API-compatible
`image-size-next@2.1.1`. That release adds forward-progress checks to the ICNS, JXL, HEIF, and JP2
parsers and regression tests for zero-sized structures. The override is exact and the npm lock pins
the registry tarball's integrity; it must not float to a tag or branch.

The repository currently contains only reviewable SVG website assets—no ICNS, JXL, HEIF, HEIC, or
AVIF input. That keeps the fixed parser paths outside today's build as a second boundary, not as a
reason to retain vulnerable code. A clean `npm ci`, `npm audit`, typecheck, production build, and
development-server smoke test pass with all three overrides.
