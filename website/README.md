# Harness public documentation

This site belongs to Harness: authored Markdown, a small Rust builder, and static HTML/CSS.
It follows Mantle's independent project-site model. It needs no Node installation, shared renderer,
private Git dependency, runtime JavaScript, network service, or deployment credential.

The separate Cargo workspace and lockfile keep documentation builds independent of the product's
private dependency graph. Rust 1.97 or newer is required; the first dependency fetch uses crates.io.

## Validate and preview

From the repository root:

```console
cargo test --manifest-path website/Cargo.toml --locked
cargo run --manifest-path website/Cargo.toml --locked -- check
cargo run --manifest-path website/Cargo.toml --locked -- build --commit "$(git rev-parse HEAD)" --preview
```

Preview builds write only to `website/build`. Serve that directory at `/harness/` with any local
static file server; the project base is intentional. Preview output has no publication manifest.
The builder refuses unknown output files rather than silently publishing leftovers; if switching
from a production build to preview, remove the previous `.well-known/b10x-site.json` explicitly.

## Exact production artifact

```console
cargo run --manifest-path website/Cargo.toml --locked -- build --commit "$(git rev-parse HEAD)"
```

Production builds refuse a commit other than checkout HEAD, any uncommitted website or build-workflow
input, undeclared documentation, broken local links or anchors, raw Markdown HTML, private source
links, and unsafe output paths. The output includes `.well-known/b10x-site.json` with the exact
commit, `/harness/` base, and a digest of the ordered output paths and bytes (excluding that manifest).
No build timestamp enters the artifact. The pinned read-only `pages.yml` validates pull requests and
uploads `b10x-project-site` for main pushes; it has no publication permission.

## Content and navigation

`docs/` is the explicit public source set. `src/main.rs` declares the full navigation; every Markdown
file must appear there, and every declared page must exist. Standard Markdown tables, fenced code,
links and blockquotes are supported. Local links name `.md` sources and resolve to project-local
HTML routes. The product's `cargo xtask website-contract` separately checks release and CLI coverage;
`cargo xtask toolchain-docs --check` owns the generated toolchain reference.

`index.html` is the landing-page body. `styles.css` provides the shared layout. Nothing outside the
explicit source and asset allowlist is copied into the output. Keep internal plans and operational
evidence out of these sources.

## Delivery boundary

This change prepares an independent static artifact for `/harness/`; it does **not** deploy it or
switch the live route. The existing generated `b10x.docs.yaml` and `b10x-docs-*` workflows remain
unchanged as legacy delivery integration: `/docs/harness/` is still their canonical route, and the
project Pages workflow remains a redirect façade. Those files are not dependencies of this builder.
Retiring the legacy route and arranging independent artifact delivery requires coordinated publisher
configuration. Report publication only after the live site serves the intended commit provenance.
