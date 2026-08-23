//! A bounded, read-only view of one directory, published as three tools.
//!
//! Read-only on purpose. This is what proves the loop end to end against a real endpoint without
//! any run being able to change a file, so a first live run costs inference and nothing else.
//! Anything that writes or executes belongs behind its own gate, not here.

use std::fs;
use std::path::{Path, PathBuf};

use harness_wire::{Approval, ToolCall, ToolName, ToolOutcome, ToolPort, ToolSpec};
use serde_json::{Value, json};

pub const LIST_TOOL: &str = "workspace_list";
pub const READ_TOOL: &str = "workspace_read";
pub const GREP_TOOL: &str = "workspace_grep";

const MAX_LIST_ENTRIES: usize = 500;
const MAX_READ_BYTES: u64 = 64 * 1024;
const MAX_READ_BYTES_CEILING: u64 = 256 * 1024;
const MAX_GREP_RESULTS: usize = 200;
const MAX_GREP_FILE_BYTES: u64 = 1024 * 1024;
const MAX_GREP_DEPTH: usize = 12;

/// Directories skipped while walking. Each is either machine output or another tool's private
/// state, and including them buries the answer the person asked for.
const SKIPPED: &[&str] = &[".git", "target", "node_modules", ".venv", "__pycache__"];

pub struct WorkspaceTools {
    root: PathBuf,
    specs: Vec<ToolSpec>,
}

impl WorkspaceTools {
    /// Opens `root` as the only directory these tools can see.
    ///
    /// # Errors
    ///
    /// Returns a message when `root` is not a readable directory.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|error| format!("workspace `{}`: {error}", root.as_ref().display()))?;
        if !root.is_dir() {
            return Err(format!("workspace `{}` is not a directory", root.display()));
        }
        Ok(Self {
            root,
            specs: specs(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a caller-supplied path inside the workspace.
    ///
    /// Canonicalization happens before the containment check, so a symlink or a `..` that leaves
    /// the workspace is refused by where it *lands*, not by how it is spelled.
    fn resolve(&self, relative: &str) -> Result<PathBuf, String> {
        let candidate = self.root.join(relative);
        let canonical = candidate
            .canonicalize()
            .map_err(|error| format!("`{relative}`: {error}"))?;
        if !canonical.starts_with(&self.root) {
            return Err(format!(
                "`{relative}` resolves outside the workspace and was refused"
            ));
        }
        Ok(canonical)
    }

    fn display(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    fn list(&self, arguments: &Value) -> Result<Value, String> {
        let relative = arguments.get("path").and_then(Value::as_str).unwrap_or(".");
        let target = self.resolve(relative)?;
        if !target.is_dir() {
            return Err(format!("`{relative}` is not a directory"));
        }
        let mut entries = Vec::new();
        let mut truncated = false;
        let mut reader = fs::read_dir(&target)
            .map_err(|error| format!("`{relative}`: {error}"))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        reader.sort_by_key(std::fs::DirEntry::file_name);
        for entry in reader {
            if entries.len() >= MAX_LIST_ENTRIES {
                truncated = true;
                break;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            entries.push(json!({
                "name": name,
                "kind": if is_dir { "directory" } else { "file" },
                "bytes": entry.metadata().ok().filter(|_| !is_dir).map(|meta| meta.len()),
            }));
        }
        Ok(json!({
            "path": self.display(&target),
            "entries": entries,
            // Named, so the model knows the listing is partial rather than assuming it is whole.
            "truncated": truncated,
        }))
    }

    fn read(&self, arguments: &Value) -> Result<Value, String> {
        let relative = arguments
            .get("path")
            .and_then(Value::as_str)
            .ok_or("`path` is required")?;
        let limit = arguments
            .get("max_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(MAX_READ_BYTES)
            .min(MAX_READ_BYTES_CEILING);
        let target = self.resolve(relative)?;
        if !target.is_file() {
            return Err(format!("`{relative}` is not a file"));
        }
        let bytes = fs::read(&target).map_err(|error| format!("`{relative}`: {error}"))?;
        let total = bytes.len() as u64;
        let truncated = total > limit;
        let head = &bytes[..usize::try_from(limit.min(total)).unwrap_or(bytes.len())];
        Ok(json!({
            "path": self.display(&target),
            "bytes": total,
            "truncated": truncated,
            "text": String::from_utf8_lossy(head),
        }))
    }

    fn grep(&self, arguments: &Value) -> Result<Value, String> {
        let pattern = arguments
            .get("pattern")
            .and_then(Value::as_str)
            .filter(|pattern| !pattern.is_empty())
            .ok_or("`pattern` is required and must not be empty")?;
        let relative = arguments.get("path").and_then(Value::as_str).unwrap_or(".");
        let limit = arguments
            .get("max_results")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(MAX_GREP_RESULTS)
            .min(MAX_GREP_RESULTS);
        let root = self.resolve(relative)?;

        let mut matches = Vec::new();
        let mut truncated = false;
        let boundary = self.root.clone();
        walk(&root, &boundary, 0, &mut |file| {
            if matches.len() >= limit {
                truncated = true;
                return false;
            }
            let Ok(metadata) = file.metadata() else {
                return true;
            };
            if metadata.len() > MAX_GREP_FILE_BYTES {
                return true;
            }
            let Ok(text) = fs::read_to_string(file) else {
                // Binary or unreadable. Skipping is right; reporting each one is noise.
                return true;
            };
            for (index, line) in text.lines().enumerate() {
                if matches.len() >= limit {
                    truncated = true;
                    return false;
                }
                if line.contains(pattern) {
                    matches.push(json!({
                        "path": self.display(file),
                        "line": index + 1,
                        "text": line.chars().take(400).collect::<String>(),
                    }));
                }
            }
            true
        });

        Ok(json!({
            "pattern": pattern,
            "matches": matches,
            "truncated": truncated,
        }))
    }
}

/// Walks files under `directory`, calling `visit` until it returns `false`.
///
/// Every entry is re-checked against `boundary`. Checking only the path the caller supplied is not
/// enough: `is_dir` and `read_dir` both follow symlinks, so a link inside the workspace pointing
/// anywhere else would be walked and its contents returned under a workspace-relative name.
///
/// Depth is bounded so a deep or cyclic tree cannot hold the run open.
fn walk(directory: &Path, boundary: &Path, depth: usize, visit: &mut dyn FnMut(&Path) -> bool) {
    if depth > MAX_GREP_DEPTH {
        return;
    }
    if !contained(directory, boundary) {
        return;
    }
    if directory.is_file() {
        visit(directory);
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !contained(&path, boundary) {
            continue;
        }
        if path.is_dir() {
            if SKIPPED.contains(&name.as_ref()) {
                continue;
            }
            walk(&path, boundary, depth + 1, visit);
        } else if !visit(&path) {
            return;
        }
    }
}

/// Whether `path` really lives inside `boundary`, following every link on the way.
fn contained(path: &Path, boundary: &Path) -> bool {
    path.canonicalize()
        .is_ok_and(|resolved| resolved.starts_with(boundary))
}

fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: ToolName::new(LIST_TOOL).expect("constant tool name is valid"),
            description: "List one directory inside the workspace. Paths are relative to the \
                          workspace root; `.` is the root itself."
                .to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory relative to the workspace root."},
                },
                "additionalProperties": false,
            }),
            approval: Approval::NotRequired,
        },
        ToolSpec {
            name: ToolName::new(READ_TOOL).expect("constant tool name is valid"),
            description: "Read one text file inside the workspace. The reply says whether it was \
                          truncated, so a partial read is never mistaken for a whole file."
                .to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File relative to the workspace root."},
                    "max_bytes": {"type": "integer", "description": "Byte ceiling for this read."},
                },
                "required": ["path"],
                "additionalProperties": false,
            }),
            approval: Approval::NotRequired,
        },
        ToolSpec {
            name: ToolName::new(GREP_TOOL).expect("constant tool name is valid"),
            description: "Find a literal substring in the workspace's text files. Not a regular \
                          expression. Build output and version-control directories are skipped."
                .to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Literal substring to find."},
                    "path": {"type": "string", "description": "Directory or file to search."},
                    "max_results": {"type": "integer", "description": "Ceiling on returned matches."},
                },
                "required": ["pattern"],
                "additionalProperties": false,
            }),
            approval: Approval::NotRequired,
        },
    ]
}

impl ToolPort for WorkspaceTools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        let result = match call.name.as_str() {
            LIST_TOOL => self.list(&call.arguments),
            READ_TOOL => self.read(&call.arguments),
            GREP_TOOL => self.grep(&call.arguments),
            other => Err(format!("`{other}` is not a workspace tool")),
        };
        match result {
            Ok(output) => ToolOutcome::ok(output),
            Err(message) => ToolOutcome::failed(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_wire::CallId;
    use std::fs::File;
    use std::io::Write as _;

    fn workspace() -> (tempfile::TempDir, WorkspaceTools) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        fs::create_dir(root.join("src")).expect("mkdir");
        fs::create_dir(root.join("target")).expect("mkdir");
        File::create(root.join("README.md"))
            .expect("create")
            .write_all(b"hello harness\nsecond line\n")
            .expect("write");
        File::create(root.join("src/lib.rs"))
            .expect("create")
            .write_all(b"fn marker() {}\n")
            .expect("write");
        File::create(root.join("target/generated.rs"))
            .expect("create")
            .write_all(b"fn marker() {}\n")
            .expect("write");
        let tools = WorkspaceTools::new(root).expect("the workspace opens");
        (dir, tools)
    }

    fn call(tools: &mut WorkspaceTools, name: &str, arguments: Value) -> ToolOutcome {
        tools.call(&ToolCall {
            call_id: CallId::new("call-1").expect("valid"),
            name: ToolName::new(name).expect("valid"),
            arguments,
        })
    }

    #[test]
    fn the_published_tools_are_all_read_only() {
        let (_dir, tools) = workspace();
        assert_eq!(tools.specs().len(), 3);
        assert!(
            tools
                .specs()
                .iter()
                .all(|spec| spec.approval == Approval::NotRequired),
            "a read-only toolset needs no approval gate"
        );
    }

    #[test]
    fn listing_the_root_shows_its_entries() {
        let (_dir, mut tools) = workspace();
        let outcome = call(&mut tools, LIST_TOOL, json!({"path": "."}));
        assert!(!outcome.failed, "{:?}", outcome.output);
        let names: Vec<&str> = outcome.output["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .filter_map(|entry| entry["name"].as_str())
            .collect();
        assert!(names.contains(&"README.md"), "{names:?}");
        assert!(names.contains(&"src"), "{names:?}");
    }

    #[test]
    fn reading_a_file_returns_its_text_and_says_it_is_whole() {
        let (_dir, mut tools) = workspace();
        let outcome = call(&mut tools, READ_TOOL, json!({"path": "README.md"}));
        assert!(!outcome.failed, "{:?}", outcome.output);
        assert!(
            outcome.output["text"]
                .as_str()
                .expect("text")
                .contains("hello harness")
        );
        assert_eq!(outcome.output["truncated"], json!(false));
    }

    #[test]
    fn a_truncated_read_says_so() {
        let (_dir, mut tools) = workspace();
        let outcome = call(
            &mut tools,
            READ_TOOL,
            json!({"path": "README.md", "max_bytes": 5}),
        );
        assert_eq!(outcome.output["truncated"], json!(true));
        assert_eq!(outcome.output["text"], json!("hello"));
    }

    #[test]
    fn a_path_leaving_the_workspace_is_refused() {
        let (_dir, mut tools) = workspace();
        for path in ["../../etc/passwd", "/etc/passwd"] {
            let outcome = call(&mut tools, READ_TOOL, json!({ "path": path }));
            assert!(outcome.failed, "`{path}` must be refused");
        }
    }

    #[test]
    fn a_symlink_out_of_the_workspace_is_refused_by_where_it_lands() {
        let (dir, mut tools) = workspace();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hostname", dir.path().join("escape")).expect("symlink");
        let outcome = call(&mut tools, READ_TOOL, json!({"path": "escape"}));
        assert!(outcome.failed, "{:?}", outcome.output);
        assert!(
            outcome
                .output
                .as_str()
                .expect("message")
                .contains("outside"),
            "{:?}",
            outcome.output
        );
    }

    #[test]
    fn grep_finds_a_literal_and_skips_build_output() {
        let (_dir, mut tools) = workspace();
        let outcome = call(&mut tools, GREP_TOOL, json!({"pattern": "marker"}));
        assert!(!outcome.failed, "{:?}", outcome.output);
        let paths: Vec<&str> = outcome.output["matches"]
            .as_array()
            .expect("matches")
            .iter()
            .filter_map(|entry| entry["path"].as_str())
            .collect();
        assert_eq!(paths, vec!["src/lib.rs"], "target/ must be skipped");
    }

    #[test]
    fn grep_never_follows_a_symlink_out_of_the_workspace() {
        let (dir, mut tools) = workspace();
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().expect("a directory outside the workspace");
            fs::write(outside.path().join("secret.txt"), "SENTINEL-OUTSIDE\n").expect("write");
            std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).expect("symlink");

            let outcome = call(
                &mut tools,
                GREP_TOOL,
                json!({"pattern": "SENTINEL-OUTSIDE"}),
            );
            assert!(!outcome.failed, "{:?}", outcome.output);
            assert_eq!(
                outcome.output["matches"].as_array().expect("matches").len(),
                0,
                "grep must not read outside the workspace: {:?}",
                outcome.output
            );

            // Naming the link directly is refused for the same reason.
            let direct = call(
                &mut tools,
                GREP_TOOL,
                json!({"pattern": "SENTINEL-OUTSIDE", "path": "escape"}),
            );
            assert!(
                direct.failed
                    || direct.output["matches"]
                        .as_array()
                        .expect("matches")
                        .is_empty(),
                "{:?}",
                direct.output
            );
        }
    }

    #[test]
    fn grep_reports_its_own_ceiling() {
        let (_dir, mut tools) = workspace();
        let outcome = call(
            &mut tools,
            GREP_TOOL,
            json!({"pattern": "e", "max_results": 1}),
        );
        assert_eq!(outcome.output["truncated"], json!(true));
        assert_eq!(
            outcome.output["matches"].as_array().expect("matches").len(),
            1
        );
    }

    #[test]
    fn a_missing_required_argument_refuses_by_name() {
        let (_dir, mut tools) = workspace();
        assert!(call(&mut tools, READ_TOOL, json!({})).failed);
        assert!(call(&mut tools, GREP_TOOL, json!({"pattern": ""})).failed);
    }

    #[test]
    fn an_unknown_tool_name_refuses() {
        // A name in the published class that this port does not publish. It used to read
        // `workspace.write`, which `ToolName` now refuses to construct at all — so the test was
        // about to prove the port refuses a name nobody could have called it with.
        let (_dir, mut tools) = workspace();
        assert!(call(&mut tools, "workspace_write", json!({})).failed);
    }

    #[test]
    fn reading_a_directory_refuses_rather_than_guessing() {
        let (_dir, mut tools) = workspace();
        assert!(call(&mut tools, READ_TOOL, json!({"path": "src"})).failed);
        assert!(call(&mut tools, LIST_TOOL, json!({"path": "README.md"})).failed);
    }
}
