# Documentation build boundary

The independent builder accepts an explicit public Markdown inventory and three reviewed SVG assets.
Markdown raw HTML is refused (the generated toolchain provenance comment is discarded), local links
and fragment targets are checked, and the rendered pages reject scripts, frames, embedded objects,
forms and inline event handlers. Authored landing-page HTML receives the same rendered checks.

The browser receives static HTML and CSS, with no third-party fonts, scripts or analytics. Cargo
resolves only crates.io dependencies from `website/Cargo.lock`; the separate workspace does not
resolve Harness's private dependencies. Dependency changes require lockfile review and the builder's
format, tests, and Clippy checks.

The output is restricted to `website/build`. Symlink outputs and unrecognized leftover files are
refused. A production artifact requires clean committed inputs and an exact HEAD revision; previews
omit publication provenance. CI has only `contents: read`, persists no checkout credential, and
uploads artifacts without deploying them. Legacy shared delivery is documented in [README.md](README.md).

Report vulnerabilities through the repository's
[private security policy](https://github.com/beyond10x/harness/security/policy).
