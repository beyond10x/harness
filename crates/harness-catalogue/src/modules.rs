//! Which module files `rustfmt` rewrites when it is handed one file, judged before it starts.
//!
//! # A formatted file is judged with every module it declares
//!
//! The built-in `rust_fmt` runs `rustfmt <path>` once per named file, and rustfmt rewrites every
//! module the file declares, and every module those declare, as well as the file itself:
//! `rust_fmt` on `src/lib.rs` holding `mod generated;` rewrote `src/generated.rs` under
//! `src/generated.rs=denied`, and a file the model had just written holding
//! `#[path = "../protected/x.rs"] mod k;` reached past `protected/**=denied` the same way. So
//! before a toolchain argv whose program is `rustfmt` starts, each written path it names is read
//! for the modules it declares, resolved as rustfmt resolves them, and every module file that
//! exists is judged by [`crate::written::refusal`] as a written file, as spelled and where it
//! lands, and then read in turn.
//!
//! - The files are read only when a layer refuses a write somewhere. A scope that refuses nothing
//!   has nothing a module could reach, so a run that restricts nothing reads nothing.
//! - A named path that is not a file inside the workspace is not read here: the executor refuses
//!   one that lands outside, and rustfmt formats no directory and no missing file.
//!
//! # How rustfmt finds a module
//!
//! rustfmt 1.10.0 (`rust-lang/rust` at `b940084d7e`, `src/tools/rustfmt/src/`), with the paths
//! it shares with rustc from `compiler/rustc_expand/src/module.rs`, each rule measured against
//! that rustfmt. A module's directory is where the modules it declares are looked for. A *root*
//! looks there directly; a *non-root* named `f` looks in `<directory>/f` first.
//!
//! - A named file's directory is its own, and it is a root, whatever its name, unless a directory
//!   of its stem stands beside it, which makes it a non-root (`lib.rs`,
//!   `Input::to_directory_ownership`, and `formatting.rs`).
//! - `mod x;` in a root resolves to `<directory>/x.rs` or `<directory>/x/mod.rs`. In a non-root
//!   `f` it resolves beneath `<directory>/f`, and when neither file exists there, beside the file
//!   as a root's would: rustfmt's own fallback (`parse/session.rs`, `default_submod_path`), which
//!   rustc does not have. `x.rs` is then a non-root named `x`, and `x/mod.rs` a root
//!   (`rustc_expand::module::default_submod_path`). A raw name is read without its `r#`, so
//!   `mod r#match;` resolves to `match.rs`.
//! - `#[path = "p"] mod x;` resolves to `<the declaring file's directory>/p`, in a root or not,
//!   and that file is a root (`modules.rs`, `find_external_module`; `mod_file_path`).
//! - An inline `mod x { … }` declares its modules in `<directory>/x`, and in a non-root `f` in
//!   `<directory>/f/x` — unless `<directory>/f` exists and `<directory>/f/x` does not, where
//!   rustfmt looks in `<directory>/f` (`modules.rs`, `push_inline_mod_directory`). With
//!   `#[path = "p"]` the path names that directory, `<directory>/p`, and the relative offset is
//!   dropped.
//! - A module whose file does not exist is not judged: rustfmt then refuses to format anything at
//!   all (measured). Where `x.rs` and `x/mod.rs` both exist, which rustfmt also refuses, both are
//!   judged.
//!
//! Two departures, both of which only refuse more: a `mod` in a function body or another block is
//! followed, where rustfmt follows only module items, and `#[rustfmt::skip]` is not honoured.
//!
//! # What cannot be told from the source is refused
//!
//! Each refusal names the declaring file and line and ends by telling the caller to name files
//! that declare no refused module:
//!
//! - a `#[path]` whose value is not a plain string literal — a macro such as `concat!`, an escape,
//!   a byte string or another form. rustfmt resolves `#[path = concat!(…)] mod k;` as if the
//!   attribute were absent (measured), and rustc refuses to build it (`mod_file_path_from_attr`);
//! - any other attribute on a module that carries `path`: rustfmt follows every `path = "…"` it
//!   finds inside one, `cfg_attr` or not, as well as the module's own file
//!   (`find_mods_outside_of_ast`, measured with `cfg_attr` and with an attribute of no known
//!   meaning);
//! - an inner attribute that carries `path`, which inside an inline module moves its directory
//!   (measured);
//! - a `mod` inside a macro invocation. rustfmt follows the modules in `cfg_if!` and
//!   `cfg_select!` (`is_cfg_if`, `is_cfg_select`, measured), and which macro is which cannot be
//!   told from a name. A `macro_rules!` definition is not an invocation and rustfmt follows nothing
//!   in it (measured), so its body is not read;
//! - a `mod` where no module can be declared, or one not followed by a name and then `;` or a
//!   body;
//! - a file that cannot be read, or not as Rust tokens;
//! - a module file that lands outside the workspace or cannot be resolved;
//! - more than [`MAX_FILES`] files, a module more than [`MAX_DEPTH`] files below the named one, or
//!   more than [`MAX_BYTES`] bytes of source.
//!
//! The source is read as tokens by `proc-macro2`'s lexer, the one `syn` uses, which skips comments
//! and the insides of strings and turns doc comments into attributes. Its lexer and its drop keep
//! their own stacks rather than recursing, and so does the walk here, so no nesting a file holds
//! can overflow the stack; `syn`'s parser recurses (2.0.119's `unary_expr` calls itself, so one
//! frame per `!`). `proc-macro2` keeps every source it lexed on the thread that lexed it, so each
//! scan runs on a thread of its own and lets them go with it.
//!
//! Judged when the call is made: a module file created between the judgement and the run is not
//! seen.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{Delimiter, Group, Ident, TokenStream, TokenTree};
use tool_toolchain::executor::Plan;

use crate::Scope;

/// How many files one scan reads, the named file included, before the call is refused instead.
///
/// The crate with the most source files among the 1,637 in this host's registry, `web-sys`
/// 0.3.106, holds 1,727; this leaves more than four times that.
pub(crate) const MAX_FILES: usize = 8_192;

/// How many files below the named one a module may be declared before the call is refused
/// instead.
///
/// The deepest module among those crates, `libc` 0.2.191's
/// `src/new/glibc/sysdeps/unix/linux/sparc/bits/socket_type.rs`, is eight declarations below
/// `src/lib.rs`; this leaves four times that.
pub(crate) const MAX_DEPTH: usize = 32;

/// How many bytes of source one scan reads before the call is refused instead.
///
/// The largest of those crates, `windows` 0.54.0, holds 146,420,344 bytes of `.rs` source; this
/// leaves more than three times that, and stays far below the four GiB of offsets `proc-macro2`
/// can number on one thread.
pub(crate) const MAX_BYTES: u64 = 512 * 1024 * 1024;

/// What every refusal of a formatting call for its modules tells the caller to do instead.
const INSTEAD: &str = "Name the files to format, none of which declares a refused module.";

/// How one scan is bounded.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    files: usize,
    depth: usize,
    bytes: u64,
}

const BOUNDS: Bounds = Bounds {
    files: MAX_FILES,
    depth: MAX_DEPTH,
    bytes: MAX_BYTES,
};

/// Every written path `plan` hands to `rustfmt`, once each: the files whose modules are judged.
pub(crate) fn formatted(plan: &Plan) -> Vec<String> {
    let mut named: Vec<String> = Vec::new();
    for group in &plan.groups {
        for argv in &group.plan.argv {
            let Some((program, arguments)) = argv.split_first() else {
                continue;
            };
            if Path::new(program).file_name() != Some("rustfmt".as_ref()) {
                continue;
            }
            for argument in arguments {
                if group.plan.writes.contains(argument) && !named.contains(argument) {
                    named.push(argument.clone());
                }
            }
        }
    }
    named
}

/// Why `rustfmt named`, run in `tree`, may not rewrite a module `named` declares under the run's
/// layer or the step's, or [`None`] if every module it reaches is admitted (the module's own
/// description). `named` itself is judged by the caller, as every written path is.
pub(crate) fn refusal(layers: [&Scope; 2], tree: &Path, named: &str) -> Option<String> {
    refusal_within(layers, tree, named, BOUNDS)
}

fn refusal_within(layers: [&Scope; 2], tree: &Path, named: &str, bounds: Bounds) -> Option<String> {
    let rule = layers
        .into_iter()
        .find_map(|layer| layer.first_refusing("file.write"))?
        .declared();
    std::thread::scope(|threads| {
        threads
            .spawn(|| scan(layers, tree, named, &rule, bounds).err())
            .join()
            .unwrap_or_else(|_| {
                Some(format!(
                    "the modules `{named}` declares could not be read to the end; while `{rule}` \
                     refuses writes, `rustfmt {named}` runs only when every module it reaches is \
                     judged. {INSTEAD}"
                ))
            })
    })
}

/// Reads `named` and every module it reaches, refusing at the first thing refused.
fn scan(
    layers: [&Scope; 2],
    tree: &Path,
    named: &str,
    rule: &str,
    bounds: Bounds,
) -> Result<(), String> {
    let Ok(root) = tree.canonicalize() else {
        return Err(format!(
            "the workspace `{}` cannot be resolved, so the modules `{named}` declares cannot be \
             judged; while `{rule}` refuses writes, `rustfmt {named}` runs only when every module \
             it reaches is judged. {INSTEAD}",
            tree.display()
        ));
    };
    match root.join(named).canonicalize() {
        Ok(landing) if landing.starts_with(&root) && landing.is_file() => {}
        _ => return Ok(()),
    }
    let route = PathBuf::from(named);
    let directory = route.parent().map(Path::to_path_buf).unwrap_or_default();
    let relative = route
        .file_stem()
        .filter(|stem| root.join(&directory).join(stem).is_dir())
        .map(|stem| stem.to_string_lossy().into_owned());
    let mut scan = Scan {
        layers,
        root,
        named,
        rule,
        bounds,
        files: 1,
        bytes: 0,
        seen: BTreeSet::new(),
        pending: VecDeque::new(),
    };
    if scan.files > bounds.files {
        return Err(scan.past_files());
    }
    let directory = Directory {
        path: directory,
        relative,
    };
    scan.seen
        .insert((route.clone(), directory.relative.clone()));
    scan.pending.push_back(File {
        route,
        directory,
        depth: 0,
        declared: None,
    });
    scan.run()
}

/// Where the modules a file or an inline module declares are looked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Directory {
    /// The directory, as rustfmt spells it: relative to the workspace root unless a `#[path]`
    /// made it absolute.
    path: PathBuf,
    /// The name of a non-root file, beneath whose directory of that name its modules are looked
    /// for first; [`None`] for a root.
    relative: Option<String>,
}

/// One file a scan is still to read.
struct File {
    /// Where rustfmt opens it, as it spells it.
    route: PathBuf,
    /// Where the modules it declares are looked for.
    directory: Directory,
    /// How many files below the named one it is.
    depth: usize,
    /// The declaration that reached it, `file:line` and the module's name; [`None`] for the
    /// named file.
    declared: Option<(String, String)>,
}

/// One attribute waiting for the item it is on.
struct Attribute {
    stream: TokenStream,
}

/// One delimited sequence of tokens a walk is still to read.
struct Level {
    tokens: Vec<TokenTree>,
    next: usize,
    kind: Kind,
}

enum Kind {
    /// Items or statements, whose module declarations resolve in `directory`.
    Items { directory: Directory, start: Start },
    /// The body of an invocation of the macro `name`, in which any `mod` is refused.
    Macro { name: String },
}

/// What has been read of the item now being read.
struct Start {
    /// Whether the tokens since the last item, if any, are only attributes, a visibility and
    /// `unsafe`, so that a `mod` here is a declaration.
    open: bool,
    /// The outer attributes since the last item.
    attributes: Vec<Attribute>,
}

impl Start {
    /// Ends the item before the next token: a module may be declared next when the last item
    /// `ends` there (after `;` or a body), and the attributes read so far were on the last one.
    fn end(&mut self, ends: bool) {
        self.open = ends;
        self.attributes.clear();
    }
}

impl Level {
    fn items(stream: TokenStream, directory: Directory) -> Self {
        Self {
            tokens: stream.into_iter().collect(),
            next: 0,
            kind: Kind::Items {
                directory,
                start: Start {
                    open: true,
                    attributes: Vec::new(),
                },
            },
        }
    }

    fn macro_body(stream: TokenStream, name: String) -> Self {
        Self {
            tokens: stream.into_iter().collect(),
            next: 0,
            kind: Kind::Macro { name },
        }
    }
}

/// One scan of the modules one named file reaches.
struct Scan<'a> {
    layers: [&'a Scope; 2],
    /// The workspace root, canonical: where rustfmt runs.
    root: PathBuf,
    /// The written path rustfmt is handed, which every refusal names.
    named: &'a str,
    /// The first rule that refuses a write somewhere: the reason the scan is made.
    rule: &'a str,
    bounds: Bounds,
    /// Files counted so far, the named one included.
    files: usize,
    /// Bytes of source read so far.
    bytes: u64,
    /// Every file read or waiting to be, with the name it is read as a non-root under, if any: a
    /// file reached again the same way declares the same modules.
    seen: BTreeSet<(PathBuf, Option<String>)>,
    pending: VecDeque<File>,
}

impl Scan<'_> {
    /// Reads every file until none is left, or refuses at the first thing refused.
    fn run(&mut self) -> Result<(), String> {
        while let Some(file) = self.pending.pop_front() {
            let text = self.read(&file)?;
            let tokens = TokenStream::from_str(&text).map_err(|error| {
                self.whole(&format!(
                    "`{}:{}` cannot be read as Rust tokens, so the modules it declares cannot be \
                     told",
                    file.route.display(),
                    error.span().start().line
                ))
            })?;
            self.walk(&file, tokens)?;
        }
        Ok(())
    }

    /// The text of `file`, a shebang line blanked as rustfmt strips it, within the byte bound.
    fn read(&mut self, file: &File) -> Result<String, String> {
        let path = self.root.join(&file.route);
        let size = fs::metadata(&path)
            .map_err(|error| self.unreadable(file, &error))?
            .len();
        if self.bytes.saturating_add(size) > self.bounds.bytes {
            return Err(self.past_bytes());
        }
        let mut text = fs::read_to_string(&path).map_err(|error| self.unreadable(file, &error))?;
        self.bytes = self.bytes.saturating_add(text.len() as u64);
        if self.bytes > self.bounds.bytes {
            return Err(self.past_bytes());
        }
        let shebang = shebang(&text);
        text.replace_range(..shebang, &" ".repeat(shebang));
        Ok(text)
    }

    /// Every token of one file: each module it declares resolved and judged, each body entered.
    fn walk(&mut self, file: &File, tokens: TokenStream) -> Result<(), String> {
        let mut levels = vec![Level::items(tokens, file.directory.clone())];
        while let Some(level) = levels.last_mut() {
            if level.next >= level.tokens.len() {
                levels.pop();
                continue;
            }
            let entered = match &level.kind {
                Kind::Macro { name } => {
                    let token = &level.tokens[level.next];
                    level.next += 1;
                    match token {
                        TokenTree::Ident(ident) if ident == "mod" => {
                            return Err(self.undetermined(
                                file,
                                line(ident),
                                &format!("holds a `mod` inside the macro invocation `{name}!`"),
                            ));
                        }
                        TokenTree::Group(group) => {
                            Some(Level::macro_body(group.stream(), name.clone()))
                        }
                        _ => None,
                    }
                }
                Kind::Items { .. } => self.item(file, level)?,
            };
            if let Some(entered) = entered {
                levels.push(entered);
            }
        }
        Ok(())
    }

    /// Reads the token at `level.next` of a sequence of items, and the ones it takes with it;
    /// answers the level to enter next, if any.
    fn item(&mut self, file: &File, level: &mut Level) -> Result<Option<Level>, String> {
        let Level { tokens, next, kind } = level;
        let Kind::Items { directory, start } = kind else {
            unreachable!("only a sequence of items is read item by item");
        };
        let at = *next;
        let (taken, entered) = match &tokens[at] {
            TokenTree::Punct(hash) if hash.as_char() == '#' => {
                (self.attribute(file, tokens, at, start)?, None)
            }
            TokenTree::Punct(semicolon) if semicolon.as_char() == ';' => {
                start.end(true);
                (1, None)
            }
            TokenTree::Ident(keyword) if keyword == "mod" => {
                (3, self.declaration(file, tokens, at, directory, start)?)
            }
            TokenTree::Ident(name) => word(name, tokens, at, start),
            TokenTree::Group(group) => {
                start.end(group.delimiter() == Delimiter::Brace);
                let block = Directory {
                    path: directory.path.clone(),
                    relative: None,
                };
                (1, Some(Level::items(group.stream(), block)))
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => {
                start.end(false);
                (1, None)
            }
        };
        *next += taken;
        Ok(entered)
    }

    /// The attribute at `tokens[at]`, a `#`: an inner one judged, an outer one kept for the item
    /// it is on. Answers how many tokens it takes.
    fn attribute(
        &self,
        file: &File,
        tokens: &[TokenTree],
        at: usize,
        start: &mut Start,
    ) -> Result<usize, String> {
        match (tokens.get(at + 1), tokens.get(at + 2)) {
            (Some(TokenTree::Punct(bang)), Some(TokenTree::Group(group)))
                if bang.as_char() == '!' && group.delimiter() == Delimiter::Bracket =>
            {
                if carries(group.stream(), "path") {
                    return Err(self.undetermined(
                        file,
                        tokens[at].span().start().line,
                        "holds an inner attribute that carries `path`",
                    ));
                }
                start.end(true);
                Ok(3)
            }
            (Some(TokenTree::Group(group)), _) if group.delimiter() == Delimiter::Bracket => {
                if start.open {
                    start.attributes.push(Attribute {
                        stream: group.stream(),
                    });
                }
                Ok(2)
            }
            _ => {
                start.end(false);
                Ok(1)
            }
        }
    }

    /// The module declaration at `tokens[at]`, a `mod`, resolved in `directory`: an out-of-line
    /// module judged and kept to read, or the body of an inline one, answered to enter.
    fn declaration(
        &mut self,
        file: &File,
        tokens: &[TokenTree],
        at: usize,
        directory: &Directory,
        start: &mut Start,
    ) -> Result<Option<Level>, String> {
        let line = tokens[at].span().start().line;
        if !start.open {
            return Err(self.undetermined(
                file,
                line,
                "holds a `mod` where no module can be declared",
            ));
        }
        let unplaced = "holds a `mod` that is not followed by a module name and then `;` or a body";
        let Some(TokenTree::Ident(name)) = tokens.get(at + 1) else {
            return Err(self.undetermined(file, line, unplaced));
        };
        let name = unraw(name);
        let path = self.path_attribute(file, line, &name, &start.attributes)?;
        start.end(true);
        match tokens.get(at + 2) {
            Some(TokenTree::Punct(semicolon)) if semicolon.as_char() == ';' => {
                self.out_of_line(file, line, &name, directory, path)?;
                Ok(None)
            }
            Some(TokenTree::Group(body)) if body.delimiter() == Delimiter::Brace => Ok(Some(
                Level::items(body.stream(), self.inline(directory, &name, path)),
            )),
            _ => Err(self.undetermined(file, line, unplaced)),
        }
    }

    /// The first `#[path = "…"]` of a module's attributes, or the refusal of one rustfmt would
    /// read differently from its spelling.
    fn path_attribute(
        &self,
        file: &File,
        line: usize,
        name: &str,
        attributes: &[Attribute],
    ) -> Result<Option<String>, String> {
        let mut path = None;
        for attribute in attributes {
            let tokens: Vec<TokenTree> = attribute.stream.clone().into_iter().collect();
            if matches!(tokens.first(), Some(TokenTree::Ident(word)) if unraw(word) == "path") {
                let literal = match tokens.as_slice() {
                    [_, TokenTree::Punct(equals), TokenTree::Literal(literal)]
                        if equals.as_char() == '=' =>
                    {
                        plain_string(&literal.to_string())
                    }
                    _ => None,
                };
                let Some(literal) = literal else {
                    return Err(self.undetermined(
                        file,
                        line,
                        &format!(
                            "declares the module `{name}` with a `#[path]` whose value is not a \
                             plain string literal"
                        ),
                    ));
                };
                path.get_or_insert(literal);
            } else if carries(attribute.stream.clone(), "path") {
                return Err(self.undetermined(
                    file,
                    line,
                    &format!(
                        "declares the module `{name}` with an attribute that carries `path`, as \
                         `cfg_attr` can"
                    ),
                ));
            }
        }
        Ok(path)
    }

    /// Judges and keeps to read every file `mod name;` reaches from `directory`.
    fn out_of_line(
        &mut self,
        file: &File,
        line: usize,
        name: &str,
        directory: &Directory,
        path: Option<String>,
    ) -> Result<(), String> {
        let candidates = if let Some(path) = path {
            let route = directory.path.join(path);
            let parent = route.parent().map(Path::to_path_buf).unwrap_or_default();
            vec![(
                route,
                Directory {
                    path: parent,
                    relative: None,
                },
            )]
        } else {
            let beneath = match &directory.relative {
                Some(relative) => directory.path.join(relative),
                None => directory.path.clone(),
            };
            let mut candidates = defaults(&beneath, name);
            if directory.relative.is_some()
                && !candidates.iter().any(|(route, _)| self.exists(route))
            {
                candidates = defaults(&directory.path, name);
            }
            candidates
        };
        for (route, directory) in candidates {
            if self.exists(&route) {
                self.module(file, line, name, route, directory)?;
            }
        }
        Ok(())
    }

    /// Where the modules declared inside `mod name { … }` resolve, within `directory`.
    fn inline(&self, directory: &Directory, name: &str, path: Option<String>) -> Directory {
        let path = match (path, &directory.relative) {
            (Some(path), _) => directory.path.join(path),
            (None, Some(relative)) => {
                let beneath = directory.path.join(relative);
                if self.exists(&beneath) && !self.exists(&beneath.join(name)) {
                    beneath
                } else {
                    beneath.join(name)
                }
            }
            (None, None) => directory.path.join(name),
        };
        Directory {
            path,
            relative: None,
        }
    }

    /// Judges `route`, the module `name` that `file:line` declares, as spelled and where it
    /// lands, and keeps it to read with its modules resolving in `directory`.
    fn module(
        &mut self,
        file: &File,
        line: usize,
        name: &str,
        route: PathBuf,
        directory: Directory,
    ) -> Result<(), String> {
        let declared = format!("{}:{line}", file.route.display());
        let spelled = route.to_string_lossy().into_owned();
        let landing = match self.root.join(&route).canonicalize() {
            Ok(landing) if landing.starts_with(&self.root) => self.shown(&landing),
            Ok(_) => {
                return Err(self.unjudged(
                    &declared,
                    name,
                    &spelled,
                    "lands outside the workspace, where no rule can be judged",
                ));
            }
            Err(error) => {
                return Err(self.unjudged(
                    &declared,
                    name,
                    &spelled,
                    &format!("cannot be resolved in the workspace ({error})"),
                ));
            }
        };
        let rewritten = format!(
            "`{declared}` declares the module `{name}` in `{spelled}`, which `rustfmt {}` \
             rewrites as well",
            self.named
        );
        let judged =
            |path: &str| crate::written::refusal(self.layers, Some(&self.root), "file.write", path);
        if let Some(refusal) = judged(&spelled) {
            return Err(format!("{rewritten}, and {refusal} {INSTEAD}"));
        }
        if landing != spelled
            && let Some(refusal) = judged(&landing)
        {
            return Err(format!(
                "{rewritten}; `{spelled}` leads to `{landing}`, and {refusal} {INSTEAD}"
            ));
        }
        if !self
            .seen
            .insert((route.clone(), directory.relative.clone()))
        {
            return Ok(());
        }
        let depth = file.depth + 1;
        if depth > self.bounds.depth {
            return Err(format!(
                "`{declared}` declares the module `{name}` more than {} files below `{}`, deeper \
                 than modules are judged while `{}` refuses writes, so it is refused rather than \
                 judged in part. {INSTEAD}",
                self.bounds.depth, self.named, self.rule
            ));
        }
        self.files += 1;
        if self.files > self.bounds.files {
            return Err(self.past_files());
        }
        self.pending.push_back(File {
            route,
            directory,
            depth,
            declared: Some((declared, name.to_owned())),
        });
        Ok(())
    }

    /// Whether `route` names something rustfmt would open, links followed.
    fn exists(&self, route: &Path) -> bool {
        self.root.join(route).exists()
    }

    /// `path`, absolute and inside the root, as the workspace spells it.
    fn shown(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    /// A refusal for something at `file:line` that `what` describes, whose target cannot be told.
    fn undetermined(&self, file: &File, line: usize, what: &str) -> String {
        self.whole(&format!(
            "`{}:{line}` {what}, so the file rustfmt rewrites there cannot be told from the \
             source",
            file.route.display()
        ))
    }

    /// A refusal for the module `name` in `spelled`, declared at `declared`, which `why` cannot
    /// be judged.
    fn unjudged(&self, declared: &str, name: &str, spelled: &str, why: &str) -> String {
        self.whole(&format!(
            "`{declared}` declares the module `{name}` in `{spelled}`, which `rustfmt {}` \
             rewrites as well, and it {why}",
            self.named
        ))
    }

    /// A refusal for `file`, which cannot be read.
    fn unreadable(&self, file: &File, error: &std::io::Error) -> String {
        let what = match &file.declared {
            None => format!("`{}` cannot be read ({error})", file.route.display()),
            Some((declared, name)) => format!(
                "`{}`, the module `{name}` that `{declared}` declares, cannot be read ({error})",
                file.route.display()
            ),
        };
        self.whole(&what)
    }

    /// A refusal for more files than the file bound.
    fn past_files(&self) -> String {
        format!(
            "`rustfmt {}` reaches more than {} files, more than are judged while `{}` refuses \
             writes, so it is refused rather than judged in part. {INSTEAD}",
            self.named, self.bounds.files, self.rule
        )
    }

    /// A refusal for more source than the byte bound.
    fn past_bytes(&self) -> String {
        format!(
            "`rustfmt {}` reaches more than {} bytes of module source, more than is judged while \
             `{}` refuses writes, so it is refused rather than judged in part. {INSTEAD}",
            self.named, self.bounds.bytes, self.rule
        )
    }

    /// A refusal whose reason `why` describes.
    fn whole(&self, why: &str) -> String {
        format!(
            "{why}; while `{}` refuses writes, `rustfmt {}` runs only when every module it \
             reaches is judged. {INSTEAD}",
            self.rule, self.named
        )
    }
}

/// The word at `tokens[at]`: `pub` or `unsafe` before an item, a macro invocation whose body is
/// answered to enter, a `macro_rules!` definition passed over, or any other word. Answers how
/// many tokens it takes.
fn word(
    word: &Ident,
    tokens: &[TokenTree],
    at: usize,
    start: &mut Start,
) -> (usize, Option<Level>) {
    if start.open && (word == "pub" || word == "unsafe") {
        let restricted = word == "pub"
            && matches!(
                tokens.get(at + 1),
                Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis
            );
        return (if restricted { 2 } else { 1 }, None);
    }
    let invoked = match (tokens.get(at + 1), tokens.get(at + 2), tokens.get(at + 3)) {
        (Some(TokenTree::Punct(bang)), Some(TokenTree::Group(body)), _)
            if bang.as_char() == '!' =>
        {
            Some((3, body))
        }
        (Some(TokenTree::Punct(bang)), Some(TokenTree::Ident(_)), Some(TokenTree::Group(body)))
            if bang.as_char() == '!' =>
        {
            Some((4, body))
        }
        _ => None,
    };
    let Some((taken, body)) = invoked else {
        start.end(false);
        return (1, None);
    };
    start.end(body.delimiter() == Delimiter::Brace);
    // A definition declares nothing rustfmt follows; an invocation may.
    if word == "macro_rules" && taken == 4 {
        return (taken, None);
    }
    (
        taken,
        Some(Level::macro_body(body.stream(), word.to_string())),
    )
}

/// `<base>/<name>.rs`, a non-root named `name`, and `<base>/<name>/mod.rs`, a root: the files
/// `mod name;` may resolve to without a `#[path]`.
fn defaults(base: &Path, name: &str) -> Vec<(PathBuf, Directory)> {
    vec![
        (
            base.join(format!("{name}.rs")),
            Directory {
                path: base.to_path_buf(),
                relative: Some(name.to_owned()),
            },
        ),
        (
            base.join(name).join("mod.rs"),
            Directory {
                path: base.join(name),
                relative: None,
            },
        ),
    ]
}

/// The line `ident` starts on.
fn line(ident: &Ident) -> usize {
    ident.span().start().line
}

/// `ident` as a name, without the `r#` of a raw identifier.
fn unraw(ident: &Ident) -> String {
    let spelled = ident.to_string();
    match spelled.strip_prefix("r#") {
        Some(name) => name.to_owned(),
        None => spelled,
    }
}

/// Whether `stream` holds the identifier `word` anywhere, raw or not.
fn carries(stream: TokenStream, word: &str) -> bool {
    let mut pending = vec![stream];
    while let Some(stream) = pending.pop() {
        for token in stream {
            match token {
                TokenTree::Ident(ident) if unraw(&ident) == word => return true,
                TokenTree::Group(group) => pending.push(Group::stream(&group)),
                _ => {}
            }
        }
    }
    false
}

/// The text of `literal`, a string literal as spelled, when it holds no escape: `"…"` without a
/// backslash, or a raw string. Anything else — an escape, a byte string, a suffix — is [`None`].
fn plain_string(literal: &str) -> Option<String> {
    if let Some(text) = literal
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        return (!text.contains('\\')).then(|| text.to_owned());
    }
    let raw = literal.strip_prefix('r')?;
    let fence = raw.len() - raw.trim_start_matches('#').len();
    raw[fence..]
        .strip_prefix('"')?
        .strip_suffix(&format!("\"{}", "#".repeat(fence)))
        .map(ToOwned::to_owned)
}

/// How many bytes of `text` a shebang line takes, `0` for none: `#!` at the very start not
/// followed by `[`, or by a comment, which would make it an inner attribute (rustc's
/// `strip_shebang`, which rustfmt applies).
fn shebang(text: &str) -> usize {
    let Some(rest) = text.strip_prefix("#!") else {
        return 0;
    };
    let after = rest.trim_start();
    if after.starts_with('[') || after.starts_with("//") || after.starts_with("/*") {
        return 0;
    }
    2 + rest.find('\n').unwrap_or(rest.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScopeRule;

    fn scope(rule: &str) -> Scope {
        Scope::of(vec![ScopeRule::parse(rule).expect("a rule")])
    }

    /// A workspace holding `files`, each a path and its text; a path ending in `/` is a directory.
    fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a workspace");
        for (path, text) in files {
            if let Some(directory) = path.strip_suffix('/') {
                fs::create_dir_all(dir.path().join(directory)).expect("a directory");
                continue;
            }
            let at = dir.path().join(path);
            fs::create_dir_all(at.parent().expect("a parent")).expect("its directory");
            fs::write(at, text).expect("a file");
        }
        dir
    }

    /// Why `rustfmt src/lib.rs`, or `named`, is refused in `dir` under `rule` alone, if it is.
    fn refused(dir: &tempfile::TempDir, rule: &str, named: &str) -> Option<String> {
        refusal([&scope(rule), &Scope::default()], dir.path(), named)
    }

    /// The refusal of `rustfmt named` for the module `name` that `declared` declares in `route`,
    /// which `<route>=denied` refuses.
    fn denied(declared: &str, name: &str, route: &str, named: &str) -> String {
        format!(
            "`{declared}` declares the module `{name}` in `{route}`, which `rustfmt {named}` \
             rewrites as well, and `{route}` is outside what this run may change. Nothing here \
             writes to it; if the work needs that path, the run was scoped wrongly and a person \
             should say so. {INSTEAD}"
        )
    }

    /// Asserts that denying exactly `route` refuses `rustfmt named` for the module `name` that
    /// `declared` declares there.
    fn resolves(dir: &tempfile::TempDir, named: &str, declared: &str, name: &str, route: &str) {
        assert_eq!(
            refused(dir, &format!("{route}=denied"), named).as_deref(),
            Some(denied(declared, name, route, named).as_str()),
            "`{declared}` declares `{name}` in `{route}`"
        );
    }

    /// Asserts that denying exactly `route` refuses nothing: `rustfmt named` does not reach it.
    fn unreached(dir: &tempfile::TempDir, named: &str, route: &str) {
        assert_eq!(
            refused(dir, &format!("{route}=denied"), named),
            None,
            "`rustfmt {named}` does not reach `{route}`"
        );
    }

    /// The refusal of `rustfmt src/lib.rs` under `protected/**=denied` for `what` at `at`.
    fn undetermined(at: &str, what: &str) -> String {
        format!(
            "`{at}` {what}, so the file rustfmt rewrites there cannot be told from the source; \
             while `protected/**=denied` refuses writes, `rustfmt src/lib.rs` runs only when \
             every module it reaches is judged. {INSTEAD}"
        )
    }

    const LIB: &str = "src/lib.rs";

    #[test]
    fn mod_x_resolves_to_x_rs_or_x_mod_rs_from_a_root() {
        let dir = workspace(&[
            ("src/lib.rs", "mod a;\nmod b;\nmod r#match;\n"),
            ("src/a.rs", ""),
            ("src/b/mod.rs", ""),
            ("src/match.rs", ""),
        ]);
        resolves(&dir, LIB, "src/lib.rs:1", "a", "src/a.rs");
        resolves(&dir, LIB, "src/lib.rs:2", "b", "src/b/mod.rs");
        resolves(&dir, LIB, "src/lib.rs:3", "match", "src/match.rs");
        unreached(&dir, LIB, "src/a/mod.rs");
    }

    #[test]
    fn mod_x_in_a_non_root_resolves_beneath_its_name_and_falls_back_beside_it() {
        // `src/a.rs` is a non-root named `a`. `b` and `d` resolve beneath `src/a`; nothing named
        // `c` is there, so rustfmt falls back to `src/c.rs`, which rustc would not.
        let dir = workspace(&[
            ("src/lib.rs", "mod a;\n"),
            ("src/a.rs", "mod b;\nmod c;\nmod d;\n"),
            ("src/a/b.rs", ""),
            ("src/b.rs", ""),
            ("src/c.rs", ""),
            ("src/a/d/mod.rs", ""),
        ]);
        resolves(&dir, LIB, "src/a.rs:1", "b", "src/a/b.rs");
        resolves(&dir, LIB, "src/a.rs:2", "c", "src/c.rs");
        resolves(&dir, LIB, "src/a.rs:3", "d", "src/a/d/mod.rs");
        unreached(&dir, LIB, "src/b.rs");
    }

    #[test]
    fn a_named_file_is_a_root_unless_a_directory_of_its_stem_stands_beside_it() {
        let alone = workspace(&[("src/a.rs", "mod b;\n"), ("src/b.rs", "")]);
        resolves(&alone, "src/a.rs", "src/a.rs:1", "b", "src/b.rs");
        // Whatever its name, `lib.rs` included.
        let beside = workspace(&[
            ("src/lib.rs", "mod b;\n"),
            ("src/lib/b.rs", ""),
            ("src/b.rs", ""),
        ]);
        resolves(&beside, LIB, "src/lib.rs:1", "b", "src/lib/b.rs");
        unreached(&beside, LIB, "src/b.rs");
    }

    #[test]
    fn inline_modules_nest_their_names_and_follow_rustfmts_rule_in_a_non_root() {
        let dir = workspace(&[
            ("src/lib.rs", "mod i {\n    mod j;\n}\nmod a;\nmod q;\n"),
            ("src/i/j.rs", ""),
            (
                "src/a.rs",
                "mod i {\n    mod j;\n    mod k {\n        #[path = \"z.rs\"]\n        mod z;\n    \
                 }\n}\n",
            ),
            ("src/a/i/j.rs", ""),
            ("src/a/i/k/z.rs", ""),
            ("src/a/z.rs", ""),
            // `src/q` exists and `src/q/i` does not, so rustfmt looks in `src/q`.
            ("src/q.rs", "mod i {\n    mod j;\n}\n"),
            ("src/q/j.rs", ""),
        ]);
        resolves(&dir, LIB, "src/lib.rs:2", "j", "src/i/j.rs");
        resolves(&dir, LIB, "src/a.rs:2", "j", "src/a/i/j.rs");
        resolves(&dir, LIB, "src/a.rs:5", "z", "src/a/i/k/z.rs");
        resolves(&dir, LIB, "src/q.rs:2", "j", "src/q/j.rs");
        unreached(&dir, LIB, "src/a/z.rs");
    }

    #[test]
    fn a_path_attribute_resolves_from_the_declaring_files_directory_and_names_a_root() {
        let dir = workspace(&[
            ("src/lib.rs", "mod a;\n#[path = \"p/q.rs\"]\nmod k;\n"),
            (
                "src/a.rs",
                "#[path = \"x.rs\"]\nmod x;\nmod i {\n    #[path = r\"o.rs\"]\n    mod o;\n}\n\
                 #[path = \"d\"]\nmod e {\n    mod f;\n}\n",
            ),
            ("src/x.rs", ""),
            ("src/a/x.rs", ""),
            ("src/a/i/o.rs", ""),
            ("src/d/f.rs", ""),
            ("src/a/d/f.rs", ""),
            ("src/p/q.rs", "mod r;\n"),
            ("src/p/r.rs", ""),
            ("src/p/q/r.rs", ""),
        ]);
        resolves(&dir, LIB, "src/a.rs:2", "x", "src/x.rs");
        resolves(&dir, LIB, "src/a.rs:5", "o", "src/a/i/o.rs");
        resolves(&dir, LIB, "src/a.rs:9", "f", "src/d/f.rs");
        resolves(&dir, LIB, "src/lib.rs:3", "k", "src/p/q.rs");
        resolves(&dir, LIB, "src/p/q.rs:1", "r", "src/p/r.rs");
        for decoy in ["src/a/x.rs", "src/a/d/f.rs", "src/p/q/r.rs"] {
            unreached(&dir, LIB, decoy);
        }
    }

    #[test]
    fn where_both_files_of_a_module_exist_both_are_judged() {
        let dir = workspace(&[
            ("src/lib.rs", "mod a;\n"),
            ("src/a.rs", ""),
            ("src/a/mod.rs", ""),
        ]);
        resolves(&dir, LIB, "src/lib.rs:1", "a", "src/a.rs");
        resolves(&dir, LIB, "src/lib.rs:1", "a", "src/a/mod.rs");
    }

    #[test]
    fn a_module_file_is_judged_where_it_lands_and_one_outside_the_workspace_is_refused() {
        let dir = workspace(&[
            ("src/lib.rs", "mod alias;\nmod out;\n"),
            ("protected/kept.rs", ""),
        ]);
        std::os::unix::fs::symlink("../protected/kept.rs", dir.path().join("src/alias.rs"))
            .expect("a link to a refused file");
        assert_eq!(
            refused(&dir, "protected/**=denied", LIB).as_deref(),
            Some(
                format!(
                    "`src/lib.rs:1` declares the module `alias` in `src/alias.rs`, which `rustfmt \
                     src/lib.rs` rewrites as well; `src/alias.rs` leads to `protected/kept.rs`, \
                     and `protected/kept.rs` is outside what this run may change. Nothing here \
                     writes to it; if the work needs that path, the run was scoped wrongly and a \
                     person should say so. {INSTEAD}"
                )
                .as_str()
            )
        );
        fs::remove_file(dir.path().join("src/alias.rs")).expect("the link goes");
        fs::write(dir.path().join("src/alias.rs"), "").expect("a file instead");
        let outside = tempfile::tempdir().expect("outside the workspace");
        fs::write(outside.path().join("x.rs"), "").expect("a file outside");
        std::os::unix::fs::symlink(outside.path().join("x.rs"), dir.path().join("src/out.rs"))
            .expect("a link out");
        assert_eq!(
            refused(&dir, "protected/**=denied", LIB).as_deref(),
            Some(
                format!(
                    "`src/lib.rs:2` declares the module `out` in `src/out.rs`, which `rustfmt \
                     src/lib.rs` rewrites as well, and it lands outside the workspace, where no \
                     rule can be judged; while `protected/**=denied` refuses writes, `rustfmt \
                     src/lib.rs` runs only when every module it reaches is judged. {INSTEAD}"
                )
                .as_str()
            )
        );
    }

    #[test]
    fn every_declaration_whose_file_cannot_be_told_from_the_source_is_refused_by_line() {
        let cases = [
            (
                "#[path = concat!(\"u\", \".rs\")]\nmod k;\n",
                undetermined(
                    "src/lib.rs:2",
                    "declares the module `k` with a `#[path]` whose value is not a plain string \
                     literal",
                ),
            ),
            (
                "#[path = \"a\\\\b.rs\"]\nmod k;\n",
                undetermined(
                    "src/lib.rs:2",
                    "declares the module `k` with a `#[path]` whose value is not a plain string \
                     literal",
                ),
            ),
            (
                "#[cfg_attr(unix, path = \"u.rs\")]\nmod k;\n",
                undetermined(
                    "src/lib.rs:2",
                    "declares the module `k` with an attribute that carries `path`, as \
                     `cfg_attr` can",
                ),
            ),
            (
                "#[my_attribute(path = \"u.rs\")]\nmod k;\n",
                undetermined(
                    "src/lib.rs:2",
                    "declares the module `k` with an attribute that carries `path`, as \
                     `cfg_attr` can",
                ),
            ),
            (
                "mod i {\n    #![path = \"e\"]\n    mod j;\n}\n",
                undetermined(
                    "src/lib.rs:2",
                    "holds an inner attribute that carries `path`",
                ),
            ),
            (
                "cfg_if::cfg_if! {\n    if #[cfg(unix)] {\n        mod u;\n    }\n}\n",
                undetermined(
                    "src/lib.rs:3",
                    "holds a `mod` inside the macro invocation `cfg_if!`",
                ),
            ),
            (
                "fn f() {}\nconst X: u8 = mod;\n",
                undetermined(
                    "src/lib.rs:2",
                    "holds a `mod` where no module can be declared",
                ),
            ),
            (
                "mod k = 1;\n",
                undetermined(
                    "src/lib.rs:1",
                    "holds a `mod` that is not followed by a module name and then `;` or a body",
                ),
            ),
        ];
        for (source, expected) in cases {
            let dir = workspace(&[("src/lib.rs", source), ("src/u.rs", ""), ("src/k.rs", "")]);
            assert_eq!(
                refused(&dir, "protected/**=denied", LIB).as_deref(),
                Some(expected.as_str()),
                "{source}"
            );
        }
    }

    #[test]
    fn a_module_file_that_cannot_be_read_is_refused_rather_than_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let dir = workspace(&[
            ("src/lib.rs", "mod locked;\nmod a;\n"),
            ("src/locked.rs", ""),
            ("src/a.rs", "fn f() {\n"),
        ]);
        let locked = dir.path().join("src/locked.rs");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("locked");
        // A process that reads it anyway (root) has nothing to show for the lock.
        let readable = fs::read(&locked).is_ok();
        let refusal = refused(&dir, "protected/**=denied", LIB);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).expect("unlocked");
        if !readable {
            assert_eq!(
                refusal.as_deref(),
                Some(
                    format!(
                        "`src/locked.rs`, the module `locked` that `src/lib.rs:1` declares, \
                         cannot be read (Permission denied (os error 13)); while \
                         `protected/**=denied` refuses writes, `rustfmt src/lib.rs` runs only \
                         when every module it reaches is judged. {INSTEAD}"
                    )
                    .as_str()
                )
            );
        }
        // Read last, so its refusal is the one met once the locked file reads.
        assert_eq!(
            refused(&dir, "protected/**=denied", LIB).as_deref(),
            Some(
                format!(
                    "`src/a.rs:1` cannot be read as Rust tokens, so the modules it declares \
                     cannot be told; while `protected/**=denied` refuses writes, `rustfmt \
                     src/lib.rs` runs only when every module it reaches is judged. {INSTEAD}"
                )
                .as_str()
            )
        );
    }

    #[test]
    fn a_macro_definition_a_shebang_and_a_block_are_read_as_rustfmt_reads_them() {
        // A `macro_rules!` body is not followed (rustfmt follows nothing in a definition). A
        // shebang line is not a token, and lines still count from it. A `mod` in a function body
        // is followed, which rustfmt does not do: one of the two departures that refuse more.
        let dir = workspace(&[
            (
                "src/lib.rs",
                "#!/usr/bin/env run-cargo-script\nmod w;\nmacro_rules! m {\n    () => {\n        \
                 mod u;\n    };\n}\nfn f() {\n    #[path = \"v.rs\"]\n    mod v;\n}\n",
            ),
            ("src/u.rs", ""),
            ("src/v.rs", ""),
            ("src/w.rs", ""),
        ]);
        resolves(&dir, LIB, "src/lib.rs:2", "w", "src/w.rs");
        unreached(&dir, LIB, "src/u.rs");
        resolves(&dir, LIB, "src/lib.rs:10", "v", "src/v.rs");
    }

    #[test]
    fn a_crate_none_of_whose_modules_a_rule_refuses_is_admitted() {
        let dir = workspace(&[
            (
                "src/lib.rs",
                "//! A crate.\n#![allow(dead_code)]\n\n/// A module.\npub(crate) mod a;\n",
            ),
            (
                "src/a.rs",
                "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
            ),
        ]);
        assert_eq!(refused(&dir, "protected/**=denied", LIB), None);
        assert_eq!(refused(&dir, ".git/**=denied", LIB), None);
    }

    #[test]
    fn past_the_bound_on_files_depth_or_bytes_a_call_is_refused_and_names_it() {
        let dir = workspace(&[
            ("src/lib.rs", "mod a;\n"),
            ("src/a.rs", "mod b;\n"),
            ("src/a/b.rs", ""),
        ]);
        let layers = [&scope("protected/**=denied"), &Scope::default()];
        let within = |bounds| refusal_within(layers, dir.path(), LIB, bounds);
        let wide = Bounds {
            files: 3,
            depth: 2,
            bytes: 14,
        };
        assert_eq!(within(wide), None, "three files, two deep, fourteen bytes");
        assert_eq!(
            within(Bounds { files: 2, ..wide }).as_deref(),
            Some(
                format!(
                    "`rustfmt src/lib.rs` reaches more than 2 files, more than are judged while \
                     `protected/**=denied` refuses writes, so it is refused rather than judged in \
                     part. {INSTEAD}"
                )
                .as_str()
            )
        );
        assert_eq!(
            within(Bounds { depth: 1, ..wide }).as_deref(),
            Some(
                format!(
                    "`src/a.rs:1` declares the module `b` more than 1 files below `src/lib.rs`, \
                     deeper than modules are judged while `protected/**=denied` refuses writes, \
                     so it is refused rather than judged in part. {INSTEAD}"
                )
                .as_str()
            )
        );
        assert_eq!(
            within(Bounds { bytes: 13, ..wide }).as_deref(),
            Some(
                format!(
                    "`rustfmt src/lib.rs` reaches more than 13 bytes of module source, more than \
                     is judged while `protected/**=denied` refuses writes, so it is refused \
                     rather than judged in part. {INSTEAD}"
                )
                .as_str()
            )
        );
    }

    #[test]
    fn nothing_is_read_unless_a_layer_refuses_a_write_somewhere() {
        // A bound of no files refuses any scan at all, so an admission here is a scan that never
        // happened.
        let dir = workspace(&[
            ("src/lib.rs", "#[cfg_attr(unix, path = \"u.rs\")]\nmod k;\n"),
            ("src/k.rs", ""),
        ]);
        let none = Bounds {
            files: 0,
            depth: 0,
            bytes: 0,
        };
        let allowed = scope("**=allowed");
        for layers in [
            [&Scope::default(), &Scope::default()],
            [&allowed, &Scope::default()],
            [&allowed, &allowed],
        ] {
            assert_eq!(refusal_within(layers, dir.path(), LIB, none), None);
        }
        let partial = scope("docs/**=partial-only");
        let refusal = refusal_within([&allowed, &partial], dir.path(), LIB, none)
            .expect("the step's layer refuses a whole write somewhere");
        assert!(refusal.contains("more than 0 files"), "{refusal}");
        assert!(
            refusal.contains("`docs/**=partial-only`"),
            "the rule that made the scan happen: {refusal}"
        );
    }

    #[test]
    fn only_the_written_paths_a_rustfmt_argv_names_are_formatted() {
        let plan = |argv: Vec<Vec<&str>>, writes: Vec<&str>| tool_toolchain::executor::Group {
            provider: "rust".to_owned(),
            tool: "fmt".to_owned(),
            several: true,
            paths: Vec::new(),
            plan: tool_toolchain::CommandPlan {
                argv: argv
                    .into_iter()
                    .map(|argv| argv.into_iter().map(ToOwned::to_owned).collect())
                    .collect(),
                writes: writes.into_iter().map(ToOwned::to_owned).collect(),
                empty_stdout: false,
            },
        };
        let plan = Plan {
            groups: vec![
                plan(
                    vec![
                        vec!["rustfmt", "a.rs"],
                        vec!["gofmt", "-w", "b.go"],
                        vec!["/opt/rust/bin/rustfmt", "--check", "c.rs", "d.rs"],
                        vec!["rustfmt", "a.rs"],
                    ],
                    vec!["a.rs", "b.go", "d.rs"],
                ),
                plan(vec![vec!["rustfmt", "e.rs"]], vec![]),
            ],
        };
        assert_eq!(formatted(&plan), ["a.rs", "d.rs"]);
    }
}
