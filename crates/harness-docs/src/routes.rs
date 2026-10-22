//! The `b10x-project-routes/v1` inventory of a built site: every page with its sorted rendered
//! element IDs, stamped with the commit the site was built from. The organization website reads it
//! to check links into this site.
//!
//! Docusaurus writes a page at `x.html` (`trailingSlash: false`), a directory index at
//! `x/index.html`, and a client redirect wherever a page moved or a trailing-slash form exists.
//! A redirect page is not a route: it carries no content of its own.
//!
//! Every route is listed in its `/x/` form, which the website requires of an independent route
//! inventory: `/harness/docs/<page>/`.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use crate::{Context, Result, ensure};
use serde_json::json;

/// Where the site is served, with both slashes.
pub const BASE: &str = "/harness/";

/// Whether `html` is a client redirect rather than a page: a refresh, or the copy docs-system
/// writes at `x/index.html` that sends `/x/` on to `/x`.
fn is_redirect(html: &str) -> bool {
    html.contains("http-equiv=\"refresh\"")
        || html.contains("http-equiv=refresh")
        || html.contains("<!-- b10x-trailing-slash-copy -->")
}

/// Every value of an `id` attribute in `html`, quoted or not.
#[must_use]
pub fn ids(html: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = html;
    while let Some(at) = rest.find(" id=") {
        rest = &rest[at + 4..];
        let value = match rest.as_bytes().first() {
            Some(quote @ (b'"' | b'\'')) => {
                let quote = char::from(*quote);
                rest[1..].split(quote).next().unwrap_or_default()
            }
            _ => rest
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()
                .unwrap_or_default(),
        };
        if !value.is_empty() {
            found.insert(value.to_owned());
        }
    }
    found
}

/// The route a built file answers, or `None` for a file that is no page.
fn route(relative: &str) -> Option<String> {
    if relative == "404.html" {
        return None;
    }
    if let Some(dir) = relative.strip_suffix("index.html") {
        return Some(format!("{BASE}{dir}"));
    }
    relative
        .strip_suffix(".html")
        .map(|page| format!("{BASE}{page}/"))
}

fn pages(site: &Path, dir: &Path, out: &mut BTreeMap<String, BTreeSet<String>>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .collect::<std::result::Result<_, _>>()?;
    entries.sort_by_key(fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            pages(site, &path, out)?;
            continue;
        }
        let relative = path
            .strip_prefix(site)?
            .to_str()
            .context("non-UTF-8 path in the built site")?
            .replace('\\', "/");
        let Some(route) = route(&relative) else {
            continue;
        };
        let html = fs::read_to_string(&path)?;
        if is_redirect(&html) {
            continue;
        }
        ensure!(
            out.insert(route.clone(), ids(&html)).is_none(),
            "two built files answer {route}"
        );
    }
    Ok(())
}

/// The inventory of the built site at `site`, as written to `.well-known/b10x-routes.json`.
///
/// # Errors
///
/// When the site cannot be read, has no landing page, or two files answer one route.
pub fn inventory(site: &Path, commit: &str) -> Result<String> {
    let mut found = BTreeMap::new();
    pages(site, site, &mut found)?;
    ensure!(
        found.contains_key(BASE),
        "the built site has no landing page"
    );
    let routes: Vec<_> = found
        .into_iter()
        .map(|(path, anchors)| json!({"path": path, "anchors": anchors}))
        .collect();
    Ok(serde_json::to_string_pretty(&json!({
        "schema": "b10x-project-routes/v1",
        "repository": "harness",
        "commit": commit,
        "baseUrl": BASE,
        "routes": routes,
    }))? + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_read_quoted_and_unquoted() {
        let html = r##"<h2 id="the-guard">x</h2><main id=main class=a><a href="#top">"##;
        assert_eq!(
            ids(html).into_iter().collect::<Vec<_>>(),
            ["main", "the-guard"]
        );
    }

    #[test]
    fn files_become_trailing_slash_routes() {
        assert_eq!(route("index.html").as_deref(), Some("/harness/"));
        assert_eq!(route("docs/index.html").as_deref(), Some("/harness/docs/"));
        assert_eq!(
            route("docs/reference/cli.html").as_deref(),
            Some("/harness/docs/reference/cli/")
        );
        assert_eq!(route("404.html"), None);
        assert_eq!(route("assets/a.js"), None);
    }
}
