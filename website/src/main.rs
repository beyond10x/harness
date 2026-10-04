//! Repository-owned, script-free public documentation with exact source provenance.
use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use pulldown_cmark::{Event, Options, Parser as Markdown, Tag, TagEnd, html};
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const BASE: &str = "/harness/";
const ORIGIN: &str = "https://beyond10x.github.io";
const PRIVATE_REFERENCES: &[&str] = &[
    "docs/design",
    ".engineering",
    "STATUS.md",
    "ROADMAP.md",
    "beyond10x/atlas",
    "../atlas",
];
const PAGES: &[(&str, &str, &str)] = &[
    ("index", "Overview", "Start here"),
    ("getting-started", "First read-only run", "Start here"),
    (
        "tutorials/confined-change",
        "First confined change",
        "Start here",
    ),
    ("guides/profiles", "Providers & profiles", "Guides"),
    ("guides/confinement", "Confinement", "Guides"),
    ("guides/sessions-and-events", "Sessions & events", "Guides"),
    (
        "guides/structured-runs",
        "Delegates, skills & hooks",
        "Guides",
    ),
    ("guides/workflows", "Run a workflow", "Guides"),
    ("concepts/agent-loop", "The agent loop", "Concepts"),
    (
        "concepts/tools-and-approvals",
        "Tools & approvals",
        "Concepts",
    ),
    (
        "concepts/security-boundary",
        "Security boundary",
        "Concepts",
    ),
    ("reference/cli", "Command line", "Reference"),
    ("reference/configuration", "Configuration", "Reference"),
    ("reference/toolchains", "Toolchains", "Reference"),
    ("reference/wires", "Provider wires", "Reference"),
    ("reference/workflows", "Workflow format", "Reference"),
    ("status", "Status & limitations", "Reference"),
];

#[derive(Parser)]
#[command(about = "Validate and build the independent Harness documentation")]
struct Cli {
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Validate all public pages, local links, anchors and assets without writing.
    Check,
    /// Build into website/build; production builds require exact clean source inputs.
    Build {
        #[arg(long, default_value = "build")]
        out: PathBuf,
        #[arg(long)]
        commit: String,
        /// Build uncommitted content for local review, without publication provenance.
        #[arg(long)]
        preview: bool,
    },
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn route(id: &str) -> String {
    if id == "index" {
        format!("{BASE}docs/")
    } else {
        format!("{BASE}docs/{id}/")
    }
}
fn normalize(path: &Path) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir => ensure!(out.pop(), "path escapes its root"),
            _ => bail!("only relative paths are admitted"),
        }
    }
    Ok(out)
}
fn link(id: &str, target: &str) -> Result<String> {
    if target.starts_with("https://") || target.starts_with('#') {
        return Ok(target.into());
    }
    ensure!(
        !target.contains([':', '?', '\\']) && !target.starts_with('/'),
        "unsupported link {target}"
    );
    let (file, anchor) = target.split_once('#').unwrap_or((target, ""));
    ensure!(
        file.ends_with(".md"),
        "documentation link must name a Markdown source: {target}"
    );
    let source = Path::new(id).parent().unwrap_or(Path::new("")).join(file);
    let normalized = normalize(&source)?;
    let name = normalized.with_extension("").to_string_lossy().into_owned();
    ensure!(
        PAGES.iter().any(|page| page.0 == name),
        "link to undeclared public source {target}"
    );
    Ok(format!(
        "{}{}{}",
        route(&name),
        if anchor.is_empty() { "" } else { "#" },
        anchor
    ))
}
fn slug(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}
fn document(id: &str, source: &str) -> Result<(String, String, String, String)> {
    let (front, body) = source
        .strip_prefix("---\n")
        .and_then(|s| s.split_once("\n---\n"))
        .context("missing page frontmatter")?;
    let title = front
        .lines()
        .find_map(|l| l.strip_prefix("title: "))
        .context("missing title")?;
    let description = front
        .lines()
        .find_map(|l| l.strip_prefix("description: "))
        .unwrap_or(title);
    ensure!(!body.contains(":::"), "legacy admonition syntax in {id}");
    let events: Vec<_> =
        Markdown::new_ext(body, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH).collect();
    let mut rendered = Vec::new();
    let mut toc = String::new();
    let mut seen = BTreeMap::<String, usize>::new();
    let has_h1 = events.iter().any(|event| {
        matches!(
            event,
            Event::Start(Tag::Heading {
                level: pulldown_cmark::HeadingLevel::H1,
                ..
            })
        )
    });
    if !has_h1 {
        rendered.push(Event::Html(format!("<h1>{}</h1>", escape(title)).into()));
    }
    for (index, event) in events.iter().enumerate() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let text: String = events[index + 1..]
                    .iter()
                    .take_while(|e| !matches!(e, Event::End(TagEnd::Heading(_))))
                    .filter_map(|e| match e {
                        Event::Text(s) | Event::Code(s) => Some(s.as_ref()),
                        _ => None,
                    })
                    .collect();
                let base = slug(&text);
                let count = seen.entry(base.clone()).or_default();
                let anchor = if *count == 0 {
                    base
                } else {
                    format!("{base}-{count}")
                };
                *count += 1;
                rendered.push(Event::Html(format!("<{level} id=\"{anchor}\">").into()));
                if *level == pulldown_cmark::HeadingLevel::H2 {
                    toc.push_str(&format!("<a href=\"#{anchor}\">{}</a>", escape(&text)));
                }
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id: link_id,
            }) => rendered.push(Event::Start(Tag::Link {
                link_type: *link_type,
                dest_url: link(id, dest_url)?.into(),
                title: title.clone(),
                id: link_id.clone(),
            })),
            Event::Html(value) | Event::InlineHtml(value) => {
                // Generated toolchain pages carry one public provenance comment, not executable HTML.
                ensure!(
                    value.trim().starts_with("<!--") && value.trim().ends_with("-->"),
                    "raw HTML is not admitted in public Markdown: {id}"
                );
            }
            _ => rendered.push(event.clone()),
        }
    }
    let mut content = String::new();
    html::push_html(&mut content, rendered.into_iter());
    Ok((title.into(), description.into(), content, toc))
}
fn navigation(current: &str) -> String {
    let mut output = String::new();
    let mut section = "";
    for (id, label, group) in PAGES {
        if *group != section {
            if !section.is_empty() {
                output.push_str("</div>");
            }
            output.push_str(&format!("<div class=\"nav-group\"><p>{group}</p>"));
            section = group;
        }
        output.push_str(&format!(
            "<a href=\"{}\"{}>{}</a>",
            route(id),
            if *id == current {
                " aria-current=\"page\""
            } else {
                ""
            },
            escape(label)
        ));
    }
    output.push_str("</div>");
    output
}
fn shell(title: &str, description: &str, path: &str, body: &str, class: &str) -> String {
    format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{} · Harness</title><meta name="description" content="{}"><link rel="canonical" href="{ORIGIN}{path}"><link rel="stylesheet" href="{BASE}styles.css"><link rel="icon" href="{BASE}img/favicon.svg" type="image/svg+xml"><meta name="theme-color" content="#101b1a"></head>
<body class="{class}"><a class="skip" href="#main">Skip to content</a><header class="topbar"><a class="wordmark" href="{BASE}"><span class="mark" aria-hidden="true">h</span> Harness <span class="byline">by beyond10x</span></a><nav aria-label="Primary"><a href="{BASE}docs/getting-started/">Get started</a><a href="{BASE}docs/reference/cli/">Reference</a><a href="{BASE}docs/status/">Status</a><a href="https://github.com/beyond10x/harness">GitHub <span aria-hidden="true">↗</span></a></nav></header>{body}<footer class="footer"><span>Harness · An effect is either gated or it did not happen.</span><a href="https://github.com/beyond10x/harness/security/policy">Security</a><span>Publicly readable, proprietary source.</span></footer></body></html>"##,
        escape(title),
        escape(description)
    )
}
fn files(root: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut result = BTreeMap::new();
    let mut declared = BTreeSet::new();
    for (id, _, group) in PAGES {
        declared.insert(format!("{id}.md"));
        let source = String::from_utf8(read_source(root, &format!("docs/{id}.md"))?)?;
        for forbidden in PRIVATE_REFERENCES {
            ensure!(
                !source.contains(*forbidden),
                "private/internal content in {id}: {forbidden}"
            );
        }
        let (title, description, content, toc) =
            document(id, &source).with_context(|| format!("rendering {id}"))?;
        let navigation = navigation(id);
        let body = format!(
            r##"<div class="docs-layout"><details class="mobile-menu"><summary>Browse documentation</summary><nav aria-label="Documentation">{navigation}</nav></details><aside class="sidebar"><p class="sidebar-title">Documentation</p><nav aria-label="Documentation">{navigation}</nav></aside><main id="main" class="article"><p class="eyebrow">{group}</p>{content}<div class="page-end"><a href="https://github.com/beyond10x/harness/blob/main/website/docs/{id}.md">View page source ↗</a><a href="#main">Back to top ↑</a></div></main><aside class="toc"><nav aria-label="On this page"><p>On this page</p>{toc}</nav></aside></div>"##
        );
        result.insert(
            format!("{}index.html", route(id).trim_start_matches(BASE)),
            shell(&title, &description, &route(id), &body, "docs").into_bytes(),
        );
    }
    let actual: BTreeSet<_> = source_files(&root.join("docs"))?.into_iter().collect();
    ensure!(
        actual == declared,
        "public document inventory differs from navigation: {:?}",
        actual.symmetric_difference(&declared).collect::<Vec<_>>()
    );
    let landing = String::from_utf8(read_source(root, "index.html")?)?;
    result.insert("index.html".into(), shell("One loop. Explicit effects.", "A provider-neutral agent loop. Inspect the tools, set the bounds, and keep the evidence.", BASE, &landing, "home").into_bytes());
    result.insert("styles.css".into(), read_source(root, "styles.css")?);
    for asset in ["favicon.svg", "mark.svg", "social-card.svg"] {
        result.insert(
            format!("img/{asset}"),
            read_source(root, &format!("static/img/{asset}"))?,
        );
    }
    result.insert(".nojekyll".into(), Vec::new());
    validate(&result)?;
    Ok(result)
}
fn read_source(root: &Path, relative: &str) -> Result<Vec<u8>> {
    let mut path = root.to_path_buf();
    for part in Path::new(relative).components() {
        ensure!(matches!(part, Component::Normal(_)), "invalid source path");
        path.push(part);
        ensure!(!path.is_symlink(), "symlink source refused: {relative}");
    }
    Ok(fs::read(path)?)
}
fn source_files(root: &Path) -> Result<Vec<String>> {
    fn visit(root: &Path, dir: &Path, paths: &mut Vec<String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            ensure!(
                !entry.file_type()?.is_symlink(),
                "symlink input/output refused"
            );
            if path.is_dir() {
                visit(root, &path, paths)?;
            } else {
                paths.push(path.strip_prefix(root)?.to_string_lossy().into());
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    visit(root, root, &mut paths)?;
    paths.sort();
    Ok(paths)
}
fn validate(files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let id_selector = Selector::parse("[id]").unwrap();
    let link_selector = Selector::parse("[href], [src]").unwrap();
    let mut documents = BTreeMap::new();
    for (path, bytes) in files.iter().filter(|(path, _)| path.ends_with(".html")) {
        let text = std::str::from_utf8(bytes)?;
        for forbidden in PRIVATE_REFERENCES {
            ensure!(
                !text.contains(*forbidden),
                "private/internal content in {path}: {forbidden}"
            );
        }
        ensure!(
            text.starts_with("<!doctype html>")
                && text.contains("lang=\"en\"")
                && text.contains("name=\"viewport\""),
            "missing document metadata: {path}"
        );
        let document = Html::parse_document(text);
        let mut ids = BTreeSet::new();
        for element in document.select(&id_selector) {
            let id = element.value().attr("id").unwrap();
            ensure!(
                ids.insert(id.to_string()),
                "duplicate anchor {id} in {path}"
            );
        }
        if let Some(element) = document
            .select(&Selector::parse("script, iframe, object, embed, form").unwrap())
            .next()
        {
            bail!("active content {} in {path}", element.value().name());
        }
        for element in document.select(&Selector::parse("*").unwrap()) {
            for (name, _) in element.value().attrs() {
                ensure!(!name.starts_with("on"), "event handler in {path}");
            }
        }
        documents.insert(path.clone(), (document, ids));
    }
    for (path, (document, _)) in &documents {
        for element in document.select(&link_selector) {
            let target = element
                .value()
                .attr("href")
                .or_else(|| element.value().attr("src"))
                .unwrap();
            if target.starts_with("https://") {
                continue;
            }
            let (file, fragment) = target.split_once('#').unwrap_or((target, ""));
            let destination = if file.is_empty() {
                path.clone()
            } else {
                let relative = file
                    .strip_prefix(BASE)
                    .with_context(|| format!("unexpected route {target} in {path}"))?;
                ensure!(
                    !relative.contains(['?', '\\'])
                        && !relative.split('/').any(|p| p == "." || p == ".."),
                    "unsafe route {target}"
                );
                if relative.is_empty() || relative.ends_with('/') {
                    format!("{relative}index.html")
                } else {
                    relative.to_string()
                }
            };
            ensure!(
                files.contains_key(&destination),
                "broken link {target} in {path}"
            );
            if !fragment.is_empty() {
                ensure!(
                    documents
                        .get(&destination)
                        .is_some_and(|(_, ids)| ids.contains(fragment)),
                    "broken anchor {target} in {path}"
                );
            }
        }
    }
    Ok(())
}
fn git(root: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(git_bytes(root, args)?)?.trim().into())
}
fn git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    ensure!(output.status.success(), "git {} failed", args.join(" "));
    Ok(output.stdout)
}
fn verify_commit(root: &Path, commit: &str, preview: bool) -> Result<()> {
    ensure!(
        commit.len() == 40
            && commit.bytes().all(|c| c.is_ascii_hexdigit())
            && commit != "0".repeat(40),
        "commit must be a nonzero full Git revision"
    );
    ensure!(
        git(root, &["rev-parse", "HEAD"])? == commit,
        "commit must equal checkout HEAD"
    );
    if !preview {
        ensure!(
            git(
                root,
                &[
                    "status",
                    "--porcelain",
                    "--untracked-files=all",
                    "--",
                    ".",
                    "../.github/workflows/pages.yml"
                ]
            )?
            .is_empty(),
            "publication requires clean committed website and workflow inputs; use --preview for local review"
        );
        // Compare bytes too: status alone can hide assume-unchanged/skip-worktree inputs.
        let prefix = git(root, &["rev-parse", "--show-prefix"])?;
        for file in git(
            root,
            &["ls-files", "--", ".", "../.github/workflows/pages.yml"],
        )?
        .lines()
        {
            let path = root.join(file);
            ensure!(!path.is_symlink(), "symlink source refused: {file}");
            let repository_path = normalize(&PathBuf::from(&prefix).join(file))?;
            let object = format!("{commit}:{}", repository_path.display());
            ensure!(
                fs::read(&path)? == git_bytes(root, &["show", &object])?,
                "source bytes differ from commit: {file}"
            );
        }
    }
    Ok(())
}
fn output_path(root: &Path, out: &Path) -> Result<PathBuf> {
    let destination = if out.is_absolute() {
        out.to_path_buf()
    } else {
        root.join(out)
    };
    ensure!(
        destination == root.join("build"),
        "output must be website/build; source paths and arbitrary directories are refused"
    );
    if destination.symlink_metadata().is_ok() {
        ensure!(
            destination.is_dir() && !destination.is_symlink(),
            "output must be a real directory"
        );
    }
    Ok(destination)
}
fn build(root: &Path, out: &Path, commit: &str, preview: bool) -> Result<()> {
    ensure!(
        read_source(root, "src/main.rs")? == include_bytes!("main.rs"),
        "builder source changed; rebuild the documentation binary before building a site"
    );
    let mut site = files(root)?;
    verify_commit(root, commit, preview)?;
    let out = output_path(root, out)?;
    let mut digest = Sha256::new();
    for (path, bytes) in &site {
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path);
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    if !preview {
        let provenance = serde_json::json!({"schema":"b10x-project-site/v1", "repository":"harness", "commit":commit, "baseUrl":BASE, "contentSha256":digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>()});
        site.insert(
            ".well-known/b10x-site.json".into(),
            serde_json::to_vec_pretty(&provenance)?,
        );
    }
    if out.exists() {
        for file in source_files(&out)? {
            ensure!(
                site.contains_key(&file),
                "unexpected/stale output {file}; remove that exact file before rebuilding"
            );
        }
    }
    for (path, bytes) in &site {
        let destination = out.join(path);
        fs::create_dir_all(destination.parent().unwrap())?;
        fs::write(destination, bytes)?;
    }
    println!(
        "Harness documentation: {} pages built at {}{}",
        PAGES.len() + 1,
        out.display(),
        if preview {
            " (preview; no publication provenance)"
        } else {
            ""
        }
    );
    Ok(())
}
fn main() -> Result<()> {
    let root = Path::new(ROOT);
    match Cli::parse().command {
        Action::Check => {
            files(root)?;
            println!(
                "Harness documentation: 18 pages, navigation, local links, anchors and public source boundary valid"
            );
        }
        Action::Build {
            out,
            commit,
            preview,
        } => build(root, &out, &commit, preview)?,
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_site_resolves() {
        files(Path::new(ROOT)).unwrap();
    }
    #[test]
    fn unresolved_anchor_and_wrong_base_fail() {
        let mut site = files(Path::new(ROOT)).unwrap();
        let original = site["index.html"].clone();
        site.insert(
            "index.html".into(),
            String::from_utf8(original.clone())
                .unwrap()
                .replace("href=\"#main\"", "href=\"#missing\"")
                .into(),
        );
        assert!(validate(&site).is_err());
        site.insert(
            "index.html".into(),
            String::from_utf8(original)
                .unwrap()
                .replace("/harness/styles.css", "/styles.css")
                .into(),
        );
        assert!(validate(&site).is_err());
    }
    #[test]
    fn sources_and_active_content_are_fenced() {
        assert!(link("index", "../.engineering/planning.md").is_err());
        assert!(link("index", "https://example.com").is_ok());
        assert!(document("index", "---\ntitle: Test\n---\n<script>alert(1)</script>").is_err());
        let mut site = files(Path::new(ROOT)).unwrap();
        site.insert("rogue.html".into(), b"<!doctype html><html lang=\"en\"><meta name=\"viewport\"><img onerror=\"bad()\"></html>".to_vec());
        assert!(validate(&site).is_err());
        site.insert("rogue.html".into(), b"<!doctype html><html lang=\"en\"><meta name=\"viewport\"><a href=\"https://github.com/beyond10x/harness/blob/main/.engineering/project.yaml\">Internal</a></html>".to_vec());
        assert!(validate(&site).is_err());
    }
    #[test]
    fn unsafe_outputs_are_refused() {
        let root = Path::new(ROOT);
        for path in [".", "docs", "../build", "/", "build/../docs"] {
            assert!(output_path(root, Path::new(path)).is_err());
        }
        assert!(output_path(root, Path::new("build")).is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn source_and_output_symlinks_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(temp.path().join("missing"), temp.path().join("build")).unwrap();
        assert!(output_path(temp.path(), Path::new("build")).is_err());
        std::os::unix::fs::symlink(Path::new(ROOT).join("docs"), temp.path().join("docs")).unwrap();
        assert!(read_source(temp.path(), "docs/index.md").is_err());
    }
    #[test]
    fn provenance_requires_exact_clean_inputs() {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "-q"]).unwrap();
        let website = temp.path().join("website");
        fs::create_dir(&website).unwrap();
        let root = website.as_path();
        fs::write(root.join("source"), "first").unwrap();
        git(root, &["add", "."]).unwrap();
        git(
            root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-qm",
                "fixture",
            ],
        )
        .unwrap();
        let commit = git(root, &["rev-parse", "HEAD"]).unwrap();
        verify_commit(root, &commit, false).unwrap();
        assert!(verify_commit(root, &"1".repeat(40), false).is_err());
        fs::write(root.join("source"), "changed").unwrap();
        assert!(verify_commit(root, &commit, false).is_err());
        verify_commit(root, &commit, true).unwrap();
        git(root, &["update-index", "--assume-unchanged", "source"]).unwrap();
        assert!(verify_commit(root, &commit, false).is_err());
    }
}
