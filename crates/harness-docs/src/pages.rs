//! The passive-Markdown rule of the unified documentation site, which still collects these pages
//! and refuses executable MDX: outside code, no `{`, no `import`/`export` line and no capitalised
//! tag; and every admonition title in brackets (`:::caution[Planned]`), which Docusaurus otherwise
//! prints as raw text.

/// Every problem of the page `source`, each as `<line>: <problem>`, plus an unclosed fence.
#[must_use]
pub fn problems(source: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let body_start = body_start(source);
    let mut fence: Option<(char, usize)> = None;
    for (index, line) in source.lines().enumerate().skip(body_start) {
        let number = index + 1;
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
        if let Some(c) = marker.filter(|_| indent < 4) {
            let run = trimmed.chars().take_while(|x| *x == c).count();
            if run >= 3 {
                match fence {
                    None => {
                        fence = Some((c, run));
                        continue;
                    }
                    Some((open, length))
                        if open == c && run >= length && trimmed[run..].trim().is_empty() =>
                    {
                        fence = None;
                        continue;
                    }
                    Some(_) => {}
                }
            }
        }
        if fence.is_some() {
            continue;
        }
        if is_raw_admonition(line) {
            problems.push(format!(
                "{number}: admonition title not in brackets (write `:::kind[Title]`)"
            ));
        }
        if let Some(problem) = executable(&without_code_spans(line)) {
            problems.push(format!("{number}: {problem}"));
        }
    }
    if fence.is_some() {
        problems.push("0: a code fence is never closed".to_owned());
    }
    problems
}

/// The index of the first line after the front matter, or 0 when there is none.
fn body_start(source: &str) -> usize {
    let Some(rest) = source.strip_prefix("---\n") else {
        return 0;
    };
    rest.find("\n---\n")
        .map_or(0, |end| rest[..end].lines().count() + 2)
}

/// Whether `line` matches `^:::[a-z]+ +\S`: an admonition whose title is not in brackets.
fn is_raw_admonition(line: &str) -> bool {
    let Some(rest) = line.strip_prefix(":::") else {
        return false;
    };
    let kind = rest.bytes().take_while(u8::is_ascii_lowercase).count();
    let rest = &rest[kind..];
    let spaces = rest.bytes().take_while(|b| *b == b' ').count();
    kind > 0
        && spaces > 0
        && rest[spaces..]
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace())
}

/// Where the code span whose opening run of `run` backticks precedes `after` closes: the offset
/// in `after` of the next run of exactly as many backticks.
#[must_use]
pub fn span_end(after: &str, run: usize) -> Option<usize> {
    let closing = "`".repeat(run);
    let mut search = 0;
    while let Some(at) = after[search..].find(&closing) {
        let at = search + at;
        let longer = after[at + run..].starts_with('`');
        if !longer {
            return Some(at);
        }
        search = at + run + after[at + run..].bytes().take_while(|b| *b == b'`').count();
    }
    None
}

/// `line` with the contents of every inline code span blanked out. A backtick after an odd number
/// of backslashes is escaped and opens no span, as the Markdown specification says, so what
/// follows it stays prose.
fn without_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut at = 0;
    while let Some(found) = line[at..].find('`') {
        let start = at + found;
        let backslashes = line[at..start]
            .bytes()
            .rev()
            .take_while(|b| *b == b'\\')
            .count();
        if backslashes % 2 == 1 {
            out.push_str(&line[at..=start]);
            at = start + 1;
            continue;
        }
        out.push_str(&line[at..start]);
        let run = line[start..].bytes().take_while(|b| *b == b'`').count();
        let after = &line[start + run..];
        if let Some(end) = span_end(after, run) {
            out.push_str(&" ".repeat(run * 2 + end));
            at = start + run + end + run;
        } else {
            out.push_str(&line[start..start + run]);
            at = start + run;
        }
    }
    out.push_str(&line[at..]);
    out
}

/// What the unified site would refuse in a prose line, if anything.
fn executable(prose: &str) -> Option<&'static str> {
    let trimmed = prose.trim_start();
    if trimmed.starts_with("import ")
        || trimmed.starts_with("export ")
        || trimmed.starts_with("import{")
        || trimmed.starts_with("export{")
    {
        return Some("an import or export line (write it as code)");
    }
    let bytes = prose.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        let escaped = bytes[..at]
            .iter()
            .rev()
            .take_while(|b| **b == b'\\')
            .count()
            % 2
            == 1;
        if *byte == b'{' && !escaped {
            return Some(
                "a `{` outside code, which MDX reads as an expression (escape it or write it as code)",
            );
        }
        if *byte == b'<' {
            let tag = prose[at + 1..].trim_start_matches('/');
            if tag.starts_with(|c: char| c.is_ascii_uppercase()) {
                return Some(
                    "a capitalised tag outside code, which MDX reads as a component (write it as code)",
                );
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRONT: &str = "---\ntitle: T\nsidebar_position: 1\ndescription: D\n---\n\n";

    #[test]
    fn braces_and_capitalised_tags_in_code_pass_and_in_prose_are_found() {
        let page = format!(
            "{FRONT}Use `{{\"a\": 1}}` and `<VALUE>` and ``a ` {{ b``.\n\n```json\n{{\"x\": 1}}\n```\n\n<img src=\"/a.svg\" alt=\"x\" />\n\nEscaped \\{{ is fine.\n"
        );
        assert_eq!(problems(&page), Vec::<String>::new());
        let page = format!("{FRONT}A {{ in prose.\n\nA <Value> tag.\n\nimport X from 'y';\n");
        let found = problems(&page);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].starts_with("7: a `{`"), "{found:?}");
        assert!(found[1].starts_with("9: a capitalised tag"), "{found:?}");
        assert!(found[2].starts_with("11: an import"), "{found:?}");
    }

    #[test]
    fn an_escaped_backtick_opens_no_code_span() {
        assert_eq!(problems(&format!("{FRONT}Run \\`{{x}}\\` now.\n")).len(), 1);
        assert_eq!(
            problems(&format!("{FRONT}Use \\`<Value>\\` here.\n")).len(),
            1
        );
        assert!(
            problems(&format!(
                "{FRONT}A \\\\`{{x}}` after an escaped backslash.\n"
            ))
            .is_empty()
        );
        assert!(problems(&format!("{FRONT}A \\``{{x}}` after an escaped backtick.\n")).is_empty());
    }

    #[test]
    fn raw_admonitions_and_unclosed_fences_are_found() {
        assert_eq!(
            problems(&format!("{FRONT}:::caution[Planned]\nok\n:::\n")).len(),
            0
        );
        assert_eq!(
            problems(&format!("{FRONT}:::caution Planned\n:::\n")).len(),
            1
        );
        assert_eq!(
            problems(&format!("{FRONT}```\n{{\n")),
            ["0: a code fence is never closed"]
        );
        assert!(problems(&format!("{FRONT}````md\n```\n{{ inside }}\n```\n````\n")).is_empty());
    }
}
