//! Declarative toolchain providers, resolved without executing a probe.
//!
//! A specification is operator policy: built-ins are compiled into the binary and custom files
//! are read only when a caller names them. Workspace files supply project facts and enum values;
//! they never supply a toolchain specification of their own.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BUILTIN_RUST: &str = include_str!("../builtins/rust.yaml");
const BUILTIN_GO: &str = include_str!("../builtins/go.yaml");
const BUILTIN_TASKFILE: &str = include_str!("../builtins/taskfile.yaml");
const BUILTIN_NPM: &str = include_str!("../builtins/npm.yaml");
const BUILTIN_YARN: &str = include_str!("../builtins/yarn.yaml");
const MAX_LIST_ITEMS: usize = 128;
const MAX_SPEC_BYTES: usize = 1024 * 1024;
const MAX_DESCRIPTION_BYTES: usize = 512;

/// One strict, versioned YAML document.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    version: u32,
    toolchains: Vec<ProviderSpec>,
}

/// One provider as written in YAML.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ProviderSpec {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub project: ProjectProbe,
    #[serde(default)]
    pub roots: Vec<RootSpec>,
    #[serde(default)]
    pub sandbox: SandboxSpec,
    #[serde(default)]
    pub facts: Vec<FactSpec>,
    #[serde(default)]
    pub values: Vec<ValueSpec>,
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ProjectProbe {
    #[serde(default)]
    pub all: Vec<String>,
    #[serde(default)]
    pub any: Vec<String>,
    #[serde(default)]
    pub none: Vec<String>,
    #[serde(default)]
    pub refuse_if_all: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RootSpec {
    pub name: String,
    pub mount: String,
    pub candidates: Vec<RootCandidate>,
    #[serde(default)]
    pub require: Vec<RequiredPath>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RootCandidate {
    EnvDir { var: String },
    HomeDir { path: String },
    PathProgram { program: String, ancestors: usize },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RequiredPath {
    pub path: String,
    pub kind: RequiredKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequiredKind {
    File,
    Directory,
    Executable,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct SandboxSpec {
    #[serde(default)]
    pub programs: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct FactSpec {
    pub key: String,
    pub source: FactSource,
    #[serde(default)]
    pub expose_to_model: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum FactSource {
    Literal {
        value: String,
    },
    FirstLine {
        root: String,
        path: String,
    },
    TomlField {
        root: String,
        path: String,
        section: String,
        key: String,
    },
    Regex {
        root: String,
        path: String,
        pattern: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ValueSpec {
    pub name: String,
    pub source: ValueSource,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ValueSource {
    JsonMapKeys { path: String, pointer: String },
    TaskfileTasks { files: Vec<String> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub role: Option<Role>,
    #[serde(default)]
    pub parameters: BTreeMap<String, ParameterSpec>,
    pub commands: Vec<CommandSpec>,
    #[serde(default)]
    pub writes: Vec<String>,
    #[serde(default)]
    pub workspace_writes: bool,
    #[serde(default)]
    pub empty_stdout: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Check,
    Build,
    Test,
    FmtCheck,
}

impl Role {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Build => "build",
            Self::Test => "test",
            Self::FmtCheck => "fmt_check",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ParameterSpec {
    pub kind: ParameterKind,
    #[serde(default)]
    pub required: bool,
    pub description: Option<String>,
    pub default: Option<Value>,
    pub values: Option<String>,
    pub min_items: Option<usize>,
    pub max_items: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParameterKind {
    String,
    Boolean,
    StringList,
    WorkspacePath,
    WorkspacePathList,
    Enum,
}

impl ParameterKind {
    fn name(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Boolean => "boolean",
            Self::StringList => "string-list",
            Self::WorkspacePath => "workspace-path",
            Self::WorkspacePathList => "workspace-path-list",
            Self::Enum => "enum",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CommandSpec {
    pub argv: Vec<ArgvSegment>,
    pub for_each: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ArgvSegment {
    Literal(String),
    Dynamic(DynamicArgv),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct DynamicArgv {
    pub arg: Option<String>,
    pub value: Option<String>,
    pub option: Option<OptionArg>,
    pub flag: Option<FlagArg>,
    pub rest: Option<RestArg>,
    pub each: Option<String>,
    #[serde(default)]
    pub item: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionArg {
    pub name: String,
    pub arg: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlagArg {
    pub name: String,
    pub when: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestArg {
    pub arg: String,
    #[serde(default)]
    pub separator: Option<String>,
}

/// Where a provider definition came from. Its body is deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    pub source: String,
    pub sha256: String,
}

/// One resolved read-only installation root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRoot {
    pub name: String,
    pub host: PathBuf,
    pub mount: String,
}

/// One bounded context fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedFact {
    pub key: String,
    pub value: String,
    pub expose_to_model: bool,
}

/// A dynamic tool ready for catalogue publication.
#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub name: String,
    pub description: String,
    pub role: Option<Role>,
    pub input_schema: Value,
    pub workspace_writes: bool,
    parameters: BTreeMap<String, ParameterSpec>,
    commands: Vec<CommandSpec>,
    writes: Vec<String>,
    empty_stdout: bool,
    values: BTreeMap<String, Vec<String>>,
    bound_values: BTreeMap<String, String>,
}

/// A provider selected for this project and resolved against this machine.
#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    pub name: String,
    pub description: String,
    pub roots: Vec<ResolvedRoot>,
    pub env: BTreeMap<String, String>,
    pub programs: Vec<String>,
    pub facts: Vec<ResolvedFact>,
    pub tools: Vec<ResolvedTool>,
    pub provenance: Provenance,
}

/// One or more argv calls and their declared writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    pub argv: Vec<Vec<String>>,
    pub writes: Vec<String>,
    pub empty_stdout: bool,
}

#[derive(Debug, Clone)]
struct DeclaredProvider {
    spec: ProviderSpec,
    provenance: Provenance,
}

/// Built-ins plus explicitly loaded operator extensions.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    providers: BTreeMap<String, DeclaredProvider>,
}

impl Registry {
    /// The five specifications shipped with Harness.
    ///
    /// # Errors
    ///
    /// Refuses when a compiled-in document is invalid or collides with another built-in.
    pub fn builtins() -> Result<Self, String> {
        let mut registry = Self::default();
        for (name, text) in [
            ("rust", BUILTIN_RUST),
            ("go", BUILTIN_GO),
            ("taskfile", BUILTIN_TASKFILE),
            ("npm", BUILTIN_NPM),
            ("yarn", BUILTIN_YARN),
        ] {
            registry.add_document(text, &format!("builtin:{name}"))?;
        }
        Ok(registry)
    }

    /// Adds one explicitly named custom document. Nothing calls this by workspace discovery.
    ///
    /// # Errors
    ///
    /// Refuses an unreadable, oversized, malformed, invalid, or colliding document.
    pub fn load_file(&mut self, path: &Path) -> Result<(), String> {
        let canonical = path.canonicalize().map_err(|error| {
            format!("the toolchain specification `{}`: {error}", path.display())
        })?;
        let text = std::fs::read_to_string(&canonical).map_err(|error| {
            format!(
                "reading the toolchain specification `{}`: {error}",
                canonical.display()
            )
        })?;
        self.add_document(&text, &canonical.display().to_string())
    }

    /// Parses and validates a document without retaining it.
    ///
    /// # Errors
    ///
    /// Refuses an oversized, malformed, invalid, or internally colliding document.
    pub fn validate(text: &str, source: &str) -> Result<Vec<String>, String> {
        let mut registry = Self::default();
        registry.add_document(text, source)?;
        Ok(registry.names())
    }

    fn add_document(&mut self, text: &str, source: &str) -> Result<(), String> {
        if text.len() > MAX_SPEC_BYTES {
            return Err(format!(
                "the toolchain specification `{source}` is {} bytes; the bound is {MAX_SPEC_BYTES}",
                text.len()
            ));
        }
        let document: Document = serde_yaml_ng::from_str(text)
            .map_err(|error| format!("the toolchain specification `{source}`: {error}"))?;
        if document.version != 1 {
            return Err(format!(
                "the toolchain specification `{source}` has version {}; this build accepts version 1",
                document.version
            ));
        }
        let digest = sha256(text.as_bytes());
        let mut staged = Vec::new();
        for spec in document.toolchains {
            validate_provider(&spec).map_err(|error| format!("`{source}`: {error}"))?;
            if self.providers.contains_key(&spec.name)
                || staged
                    .iter()
                    .any(|declared: &DeclaredProvider| declared.spec.name == spec.name)
            {
                return Err(format!(
                    "the toolchain provider `{}` from `{source}` collides with an existing provider",
                    spec.name
                ));
            }
            staged.push(DeclaredProvider {
                spec,
                provenance: Provenance {
                    source: source.to_owned(),
                    sha256: digest.clone(),
                },
            });
        }
        let mut trial = self.clone();
        for declared in staged {
            trial.providers.insert(declared.spec.name.clone(), declared);
        }
        trial.validate_collisions()?;
        *self = trial;
        Ok(())
    }

    fn validate_collisions(&self) -> Result<(), String> {
        let mut tools = BTreeMap::<&str, &str>::new();
        let mut mounts = BTreeMap::<&str, &str>::new();
        let mut facts = BTreeSet::new();
        for declared in self.providers.values() {
            let provider = declared.spec.name.as_str();
            for tool in &declared.spec.tools {
                if let Some(previous) = tools.insert(&tool.name, provider) {
                    return Err(format!(
                        "tool `{}` is declared by both `{previous}` and `{provider}`",
                        tool.name
                    ));
                }
            }
            for root in &declared.spec.roots {
                if let Some(previous) = mounts.insert(&root.mount, provider) {
                    return Err(format!(
                        "mount `{}` is declared by both `{previous}` and `{provider}`",
                        root.mount
                    ));
                }
            }
            for fact in &declared.spec.facts {
                let key = format!("toolchain.{provider}.{}", fact.key);
                if !facts.insert(key.clone()) {
                    return Err(format!("context fact `{key}` is declared more than once"));
                }
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    #[must_use]
    pub fn definitions(&self) -> Vec<(&ProviderSpec, &Provenance)> {
        self.providers
            .values()
            .map(|provider| (&provider.spec, &provider.provenance))
            .collect()
    }

    /// Resolves either all matching providers or the explicitly selected names.
    ///
    /// # Errors
    ///
    /// Refuses an unknown or mismatched provider, a conflicting project, or an unavailable or
    /// unsafe installation and static value source.
    pub fn resolve(
        &self,
        workspace: &Path,
        selected: Option<&[String]>,
    ) -> Result<Vec<ResolvedProvider>, String> {
        let workspace = workspace
            .canonicalize()
            .map_err(|error| format!("the workspace `{}`: {error}", workspace.display()))?;
        let mut names = match selected {
            Some(names) => names.to_vec(),
            None => self
                .providers
                .values()
                .map(|provider| {
                    project_matches(&workspace, &provider.spec.project)
                        .map(|matches| matches.then(|| provider.spec.name.clone()))
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect(),
        };
        names.sort();
        names.dedup();
        names
            .into_iter()
            .map(|name| {
                let declared = self.providers.get(&name).ok_or_else(|| {
                    format!(
                        "`{name}` is not a toolchain this build knows; available providers: {}",
                        self.names().join(", ")
                    )
                })?;
                if selected.is_some() && !project_matches(&workspace, &declared.spec.project)? {
                    return Err(format!(
                        "toolchain `{name}` does not match project markers in `{}`",
                        workspace.display()
                    ));
                }
                resolve_provider(&workspace, declared)
            })
            .collect()
    }
}

/// Deterministic Markdown reference rendered from the built-in specifications.
///
/// # Errors
///
/// Refuses if the compiled-in registry is invalid.
pub fn builtin_reference() -> Result<String, String> {
    use std::fmt::Write as _;

    let registry = Registry::builtins()?;
    let mut text = String::from(
        "---\ntitle: Toolchains\n---\n\n<!-- Generated by `cargo xtask toolchain-docs`. -->\n\n\
         Toolchain providers are discovered without executing probe commands. Dedicated tools run \
         inside the declared substrate confinement and remain available when the generic `run` \
         tool is not published. Custom specifications must be named explicitly with \
         `--toolchain-spec FILE`.\n\n",
    );
    for (provider, _) in registry.definitions() {
        let _ = writeln!(text, "## `{}`\n\n{}\n", provider.name, provider.description);
        let markers = provider
            .project
            .all
            .iter()
            .chain(&provider.project.any)
            .cloned()
            .collect::<Vec<_>>();
        if !markers.is_empty() {
            let _ = writeln!(text, "Project markers: `{}`.\n", markers.join("`, `"));
        }
        let exposed = provider
            .facts
            .iter()
            .filter(|fact| fact.expose_to_model)
            .map(|fact| format!("{}.{}", provider.name, fact.key))
            .collect::<Vec<_>>();
        if !exposed.is_empty() {
            let _ = writeln!(text, "Model context: `{}`.\n", exposed.join("`, `"));
        }
        text.push_str(
            "| Tool | Parameters | Generic role | Effects | Description |\n|---|---|---|---|---|\n",
        );
        for tool in &provider.tools {
            let role = tool.role.map_or("—", Role::name);
            let parameters = tool
                .parameters
                .iter()
                .map(|(name, parameter)| {
                    format!(
                        "`{name}: {}`{}",
                        parameter.kind.name(),
                        if parameter.required {
                            " (required)"
                        } else {
                            ""
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let effects = if tool.workspace_writes {
                "process, filesystem, workspace write; high risk"
            } else {
                "process, filesystem; high risk"
            };
            let _ = writeln!(
                text,
                "| `{}` | {} | `{}` | {} | {} |",
                tool.name,
                if parameters.is_empty() {
                    "—"
                } else {
                    &parameters
                },
                role,
                effects,
                tool.description.replace('|', "\\|")
            );
        }
        text.push('\n');
    }
    text.push_str(
        "## Custom specifications\n\nUse `toolchains validate FILE`, then pass the same file with \
         `--toolchain-spec FILE`. Custom providers are additive: provider, tool, mount, and context \
         key collisions refuse the entire file. Specifications use typed argv segments and cannot \
         contain shell strings or executable discovery probes.\n\n\
         ```yaml\nversion: 1\ntoolchains:\n  - name: example\n    description: Example checker\n    project:\n      all: [example.lock]\n    sandbox:\n      programs: [example]\n      env:\n        HOME: \"{workspace}\"\n        PATH: /usr/local/bin:/usr/bin:/bin\n    tools:\n      - name: example_test\n        description: Run the example tests.\n        role: test\n        workspace-writes: true\n        parameters:\n          package: { kind: string }\n        commands:\n          - argv: [example, test, { option: { name: --package, arg: package } }]\n```\n\n\
         Project markers and JSON/YAML values are read beneath the canonical workspace. Installation \
         roots may come from a named environment directory, a home-relative directory, or a program \
         found on `PATH`; each root declares required files and is mounted read-only beneath \
         `/toolchain`. Context facts are bounded scalar reads and enter the prompt only with \
         `expose-to-model: true`.\n",
    );
    Ok(text)
}

impl ResolvedProvider {
    #[must_use]
    pub fn entry_names(&self) -> Vec<String> {
        self.tools.iter().map(|tool| tool.name.clone()).collect()
    }

    pub fn plan(&self, name: &str, arguments: &Value) -> Option<Result<CommandPlan, String>> {
        self.tools
            .iter()
            .find(|tool| tool.name == name)
            .map(|tool| tool.plan(arguments))
    }
}

impl ResolvedTool {
    /// Compile validated structured arguments into fixed argv calls.
    ///
    /// # Errors
    ///
    /// Refuses missing, unknown, incorrectly typed, out-of-enum, or escaping path arguments.
    pub fn plan(&self, arguments: &Value) -> Result<CommandPlan, String> {
        validate_arguments(self, arguments)?;
        let mut argv = Vec::new();
        for command in &self.commands {
            if let Some(parameter) = &command.for_each {
                let items = string_list(arguments, parameter)?;
                for item in items {
                    argv.push(compile_argv(
                        command,
                        arguments,
                        &self.parameters,
                        &self.bound_values,
                        Some(&item),
                    )?);
                }
            } else {
                argv.push(compile_argv(
                    command,
                    arguments,
                    &self.parameters,
                    &self.bound_values,
                    None,
                )?);
            }
        }
        let mut writes = Vec::new();
        for parameter in &self.writes {
            if let Some(value) = arguments.get(parameter) {
                if let Some(value) = value.as_str() {
                    writes.push(value.to_owned());
                } else if let Some(items) = value.as_array() {
                    writes.extend(
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned),
                    );
                }
            }
        }
        Ok(CommandPlan {
            argv,
            writes,
            empty_stdout: self.empty_stdout,
        })
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive pass keeps every cross-field specification invariant together"
)]
fn validate_provider(spec: &ProviderSpec) -> Result<(), String> {
    legal_name(&spec.name, "provider")?;
    validate_description(&spec.description, "provider", &spec.name)?;
    if spec.tools.is_empty() {
        return Err(format!("provider `{}` declares no tools", spec.name));
    }
    if spec.tools.len() > MAX_LIST_ITEMS
        || spec.roots.len() > MAX_LIST_ITEMS
        || spec.facts.len() > MAX_LIST_ITEMS
        || spec.values.len() > MAX_LIST_ITEMS
    {
        return Err(format!(
            "provider `{}` exceeds the {MAX_LIST_ITEMS}-item declaration bound",
            spec.name
        ));
    }
    for marker in spec
        .project
        .all
        .iter()
        .chain(&spec.project.any)
        .chain(&spec.project.none)
        .chain(&spec.project.refuse_if_all)
    {
        validate_relative(marker)?;
    }
    let programs: BTreeSet<&str> = spec.sandbox.programs.iter().map(String::as_str).collect();
    if programs.len() != spec.sandbox.programs.len() {
        return Err(format!("provider `{}` repeats a program", spec.name));
    }
    for program in &spec.sandbox.programs {
        if Path::new(program).components().count() != 1 || program == "." || program == ".." {
            return Err(format!(
                "provider `{}` program `{program}` is not a bare executable name",
                spec.name
            ));
        }
    }
    for (name, value) in &spec.sandbox.env {
        validate_env(name, value)?;
    }
    let roots: BTreeSet<&str> = spec.roots.iter().map(|root| root.name.as_str()).collect();
    if roots.len() != spec.roots.len() {
        return Err(format!("provider `{}` repeats a root", spec.name));
    }
    for root in &spec.roots {
        legal_name(&root.name, "root")?;
        validate_relative(&root.mount)?;
        if root.mount == "driver" {
            return Err("mount `driver` is reserved for the explicitly staged driver".to_owned());
        }
        for required in &root.require {
            validate_relative(&required.path)?;
        }
    }
    for fact in &spec.facts {
        legal_name(&fact.key, "fact")?;
        let root = match &fact.source {
            FactSource::Literal { .. } => None,
            FactSource::FirstLine { root, path }
            | FactSource::TomlField { root, path, .. }
            | FactSource::Regex { root, path, .. } => {
                validate_relative(path)?;
                Some(root)
            }
        };
        if root.is_some_and(|root| !roots.contains(root.as_str())) {
            return Err(format!("fact `{}` names unknown root", fact.key));
        }
        if let FactSource::Regex { pattern, .. } = &fact.source {
            Regex::new(pattern).map_err(|error| format!("fact `{}` regex: {error}", fact.key))?;
        }
    }
    let values: BTreeSet<&str> = spec
        .values
        .iter()
        .map(|value| value.name.as_str())
        .collect();
    if values.len() != spec.values.len() {
        return Err(format!("provider `{}` repeats a value source", spec.name));
    }
    for value in &spec.values {
        legal_name(&value.name, "value")?;
        match &value.source {
            ValueSource::JsonMapKeys { path, pointer } => {
                validate_relative(path)?;
                if !pointer.is_empty() && !pointer.starts_with('/') {
                    return Err(format!(
                        "value `{}` has an invalid JSON pointer",
                        value.name
                    ));
                }
            }
            ValueSource::TaskfileTasks { files } => {
                if files.is_empty() {
                    return Err(format!(
                        "value `{}` names no Taskfile candidates",
                        value.name
                    ));
                }
                for file in files {
                    validate_relative(file)?;
                }
            }
        }
    }
    for tool in &spec.tools {
        legal_name(&tool.name, "tool")?;
        validate_description(&tool.description, "tool", &tool.name)?;
        if tool.name.contains('.') || tool.name.starts_with("toolchain_") {
            return Err(format!(
                "tool `{}` must use the wire-safe provider namespace and may not reserve a generic router name",
                tool.name
            ));
        }
        if tool.commands.is_empty() {
            return Err(format!("tool `{}` has no commands", tool.name));
        }
        if tool.commands.len() > MAX_LIST_ITEMS || tool.parameters.len() > MAX_LIST_ITEMS {
            return Err(format!(
                "tool `{}` exceeds the {MAX_LIST_ITEMS}-item declaration bound",
                tool.name
            ));
        }
        for (name, parameter) in &tool.parameters {
            legal_name(name, "parameter")?;
            if let Some(description) = &parameter.description {
                validate_description(description, "parameter", name)?;
            }
            if parameter.kind == ParameterKind::Enum
                && parameter
                    .values
                    .as_ref()
                    .is_none_or(|name| !values.contains(name.as_str()))
            {
                return Err(format!(
                    "tool `{}` parameter `{name}` names no enum values",
                    tool.name
                ));
            }
            if parameter
                .max_items
                .is_some_and(|maximum| maximum > MAX_LIST_ITEMS)
            {
                return Err(format!(
                    "tool `{}` parameter `{name}` exceeds {MAX_LIST_ITEMS} items",
                    tool.name
                ));
            }
        }
        for command in &tool.commands {
            validate_command(tool, command, &programs)?;
            for segment in &command.argv {
                if let ArgvSegment::Dynamic(dynamic) = segment
                    && dynamic
                        .value
                        .as_ref()
                        .is_some_and(|name| !values.contains(name.as_str()))
                {
                    return Err(format!(
                        "tool `{}` argv names unknown resolved value",
                        tool.name
                    ));
                }
            }
        }
        for write in &tool.writes {
            let Some(parameter) = tool.parameters.get(write) else {
                return Err(format!(
                    "tool `{}` write names unknown parameter `{write}`",
                    tool.name
                ));
            };
            if !matches!(
                parameter.kind,
                ParameterKind::WorkspacePath | ParameterKind::WorkspacePathList
            ) {
                return Err(format!(
                    "tool `{}` write parameter `{write}` is not a workspace path",
                    tool.name
                ));
            }
        }
    }
    Ok(())
}

fn validate_command(
    tool: &ToolSpec,
    command: &CommandSpec,
    programs: &BTreeSet<&str>,
) -> Result<(), String> {
    if command.argv.is_empty() || command.argv.len() > MAX_LIST_ITEMS {
        return Err(format!(
            "tool `{}` command argv must contain 1 to {MAX_LIST_ITEMS} segments",
            tool.name
        ));
    }
    let Some(ArgvSegment::Literal(program)) = command.argv.first() else {
        return Err(format!(
            "tool `{}` command must begin with a literal program",
            tool.name
        ));
    };
    if !programs.contains(program.as_str()) {
        return Err(format!(
            "tool `{}` starts undeclared program `{program}`",
            tool.name
        ));
    }
    if let Some(parameter) = &command.for_each {
        let Some(parameter_spec) = tool.parameters.get(parameter) else {
            return Err(format!(
                "tool `{}` for-each names unknown parameter `{parameter}`",
                tool.name
            ));
        };
        if !matches!(
            parameter_spec.kind,
            ParameterKind::StringList | ParameterKind::WorkspacePathList
        ) {
            return Err(format!(
                "tool `{}` for-each parameter `{parameter}` is not a list",
                tool.name
            ));
        }
    }
    for segment in &command.argv {
        let ArgvSegment::Dynamic(dynamic) = segment else {
            continue;
        };
        let count = usize::from(dynamic.arg.is_some())
            + usize::from(dynamic.value.is_some())
            + usize::from(dynamic.option.is_some())
            + usize::from(dynamic.flag.is_some())
            + usize::from(dynamic.rest.is_some())
            + usize::from(dynamic.each.is_some())
            + usize::from(dynamic.item);
        if count != 1 {
            return Err(format!(
                "tool `{}` has an argv segment with {count} operations",
                tool.name
            ));
        }
        let parameter = dynamic
            .arg
            .as_ref()
            .or(dynamic.each.as_ref())
            .or(dynamic.option.as_ref().map(|value| &value.arg))
            .or(dynamic.flag.as_ref().map(|value| &value.when))
            .or(dynamic.rest.as_ref().map(|value| &value.arg));
        if parameter.is_some_and(|name| !tool.parameters.contains_key(name)) {
            return Err(format!("tool `{}` argv names unknown parameter", tool.name));
        }
        let parameter_kind =
            parameter.and_then(|name| tool.parameters.get(name).map(|spec| spec.kind));
        if dynamic.arg.is_some()
            && !matches!(
                parameter_kind,
                Some(ParameterKind::String | ParameterKind::WorkspacePath | ParameterKind::Enum)
            )
        {
            return Err(format!("tool `{}` argv arg is not scalar", tool.name));
        }
        if dynamic.option.is_some()
            && !matches!(
                parameter_kind,
                Some(ParameterKind::String | ParameterKind::WorkspacePath | ParameterKind::Enum)
            )
        {
            return Err(format!("tool `{}` argv option is not scalar", tool.name));
        }
        if dynamic.flag.is_some() && parameter_kind != Some(ParameterKind::Boolean) {
            return Err(format!("tool `{}` argv flag is not boolean", tool.name));
        }
        if (dynamic.rest.is_some() || dynamic.each.is_some())
            && !matches!(
                parameter_kind,
                Some(ParameterKind::StringList | ParameterKind::WorkspacePathList)
            )
        {
            return Err(format!("tool `{}` argv expansion is not a list", tool.name));
        }
        if dynamic.item && command.for_each.is_none() {
            return Err(format!("tool `{}` uses item outside for-each", tool.name));
        }
    }
    Ok(())
}

fn legal_name(name: &str, what: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(format!(
            "{what} name `{name}` is not ASCII alphanumeric, `_`, `-`, or `.`"
        ));
    }
    Ok(())
}

fn validate_description(description: &str, what: &str, name: &str) -> Result<(), String> {
    if description.is_empty()
        || description.len() > MAX_DESCRIPTION_BYTES
        || description.contains(['\n', '\r'])
    {
        return Err(format!(
            "{what} `{name}` description must be one non-empty line of at most {MAX_DESCRIPTION_BYTES} bytes"
        ));
    }
    Ok(())
}

fn validate_relative(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "`{}` is not a contained relative path",
            path.display()
        ));
    }
    Ok(())
}

fn validate_env(name: &str, value: &str) -> Result<(), String> {
    const FORBIDDEN: &[&str] = &[
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "CREDENTIAL",
        "AUTH",
        "BEARER",
        "PROXY",
        "COOKIE",
    ];
    if name.is_empty()
        || name.len() > 128
        || !name.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_uppercase()
            } else {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
            }
        })
    {
        return Err(format!(
            "sandbox environment `{name}` is outside substrate's uppercase environment shape"
        ));
    }
    if FORBIDDEN.iter().any(|word| name.contains(word)) {
        return Err(format!(
            "sandbox environment `{name}` could carry a secret or proxy"
        ));
    }
    if value.contains("${") || value.contains("{{") {
        return Err(format!(
            "sandbox environment `{name}` uses unsupported interpolation"
        ));
    }
    Ok(())
}

fn project_matches(workspace: &Path, probe: &ProjectProbe) -> Result<bool, String> {
    if !probe.refuse_if_all.is_empty()
        && probe
            .refuse_if_all
            .iter()
            .all(|path| workspace.join(path).exists())
    {
        return Err(format!(
            "project markers conflict: {} all exist",
            probe.refuse_if_all.join(", ")
        ));
    }
    Ok(probe.all.iter().all(|path| workspace.join(path).exists())
        && (probe.any.is_empty() || probe.any.iter().any(|path| workspace.join(path).exists()))
        && probe.none.iter().all(|path| !workspace.join(path).exists()))
}

fn resolve_provider(
    workspace: &Path,
    declared: &DeclaredProvider,
) -> Result<ResolvedProvider, String> {
    let mut roots = Vec::new();
    for root in &declared.spec.roots {
        roots.push(resolve_root(root)?);
    }
    let root_map: BTreeMap<&str, &ResolvedRoot> = roots
        .iter()
        .map(|root| (root.name.as_str(), root))
        .collect();
    for program in &declared.spec.sandbox.programs {
        let installed_in_root = declared.spec.roots.iter().any(|root| {
            root.require.iter().any(|required| {
                matches!(required.kind, RequiredKind::Executable)
                    && Path::new(&required.path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        == Some(program.as_str())
            })
        });
        if !installed_in_root && find_program(program).is_none() {
            return Err(format!(
                "toolchain `{}` requires `{program}` on PATH, but discovery found no executable file",
                declared.spec.name
            ));
        }
    }
    let mut values = BTreeMap::new();
    let mut bound_values = BTreeMap::new();
    for source in &declared.spec.values {
        let resolved = resolve_values(workspace, source)?;
        if let Some(bound) = resolved.bound {
            bound_values.insert(source.name.clone(), bound);
        }
        values.insert(source.name.clone(), resolved.items);
    }
    let facts = declared
        .spec
        .facts
        .iter()
        .map(|fact| resolve_fact(fact, &root_map))
        .collect::<Result<Vec<_>, _>>()?;
    let tools = declared
        .spec
        .tools
        .iter()
        .filter(|tool| {
            tool.parameters.values().all(|parameter| {
                parameter.kind != ParameterKind::Enum
                    || parameter
                        .values
                        .as_ref()
                        .and_then(|name| values.get(name))
                        .is_some_and(|items| !items.is_empty())
            })
        })
        .map(|tool| resolve_tool(tool, &values, &bound_values))
        .collect::<Vec<_>>();
    let env = declared
        .spec
        .sandbox
        .env
        .iter()
        .map(|(name, value)| expand_env(value, &root_map).map(|value| (name.clone(), value)))
        .collect::<Result<_, _>>()?;
    Ok(ResolvedProvider {
        name: declared.spec.name.clone(),
        description: declared.spec.description.clone(),
        roots,
        env,
        programs: declared.spec.sandbox.programs.clone(),
        facts,
        tools,
        provenance: declared.provenance.clone(),
    })
}

fn resolve_root(spec: &RootSpec) -> Result<ResolvedRoot, String> {
    let mut reasons = Vec::new();
    for candidate in &spec.candidates {
        let path = match candidate {
            RootCandidate::EnvDir { var } => std::env::var_os(var).map(PathBuf::from),
            RootCandidate::HomeDir { path } => {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(path))
            }
            RootCandidate::PathProgram { program, ancestors } => {
                find_program(program).and_then(|path| {
                    path.canonicalize().ok().and_then(|mut path| {
                        for _ in 0..*ancestors {
                            path = path.parent()?.to_path_buf();
                        }
                        Some(path)
                    })
                })
            }
        };
        let Some(path) = path else {
            reasons.push(format!("candidate {candidate:?} was absent"));
            continue;
        };
        let canonical = match path.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                reasons.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        if !canonical.is_dir() || is_broad_root(&canonical) {
            reasons.push(format!(
                "{} is not an admissible directory",
                canonical.display()
            ));
            continue;
        }
        if let Some(error) = spec.require.iter().find_map(|required| {
            let path = canonical.join(&required.path);
            let valid = match required.kind {
                RequiredKind::File => path.is_file(),
                RequiredKind::Directory => path.is_dir(),
                RequiredKind::Executable => is_executable(&path),
            };
            (!valid).then(|| format!("{} is not {:?}", path.display(), required.kind))
        }) {
            reasons.push(error);
            continue;
        }
        return Ok(ResolvedRoot {
            name: spec.name.clone(),
            host: canonical,
            mount: format!("/toolchain/{}", spec.mount),
        });
    }
    Err(format!(
        "toolchain root `{}` could not be resolved: {}",
        spec.name,
        reasons.join("; ")
    ))
}

fn is_broad_root(path: &Path) -> bool {
    path == Path::new("/")
        || std::env::var_os("HOME")
            .and_then(|home| PathBuf::from(home).canonicalize().ok())
            .is_some_and(|home| home == path)
}

fn find_program(program: &str) -> Option<PathBuf> {
    let path: OsString = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|path| is_executable(path))
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn expand_env(value: &str, roots: &BTreeMap<&str, &ResolvedRoot>) -> Result<String, String> {
    let mut expanded = value.replace("{workspace}", "/workspace");
    while let Some(start) = expanded.find("{mount:") {
        let tail = &expanded[start + 7..];
        let end = tail
            .find('}')
            .ok_or_else(|| format!("unclosed mount placeholder in `{value}`"))?;
        let name = &tail[..end];
        let mount = roots
            .get(name)
            .ok_or_else(|| format!("unknown mount `{name}` in `{value}`"))?
            .mount
            .clone();
        expanded.replace_range(start..start + 8 + end, &mount);
    }
    if expanded.contains('{') || expanded.contains('}') {
        return Err(format!("unsupported placeholder in `{value}`"));
    }
    Ok(expanded)
}

fn resolve_fact(
    fact: &FactSpec,
    roots: &BTreeMap<&str, &ResolvedRoot>,
) -> Result<ResolvedFact, String> {
    let value = match &fact.source {
        FactSource::Literal { value } => value.clone(),
        FactSource::FirstLine { root, path } => read_root(roots, root, path)?
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(str::trim)
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("fact `{}` found no non-empty line", fact.key))?,
        FactSource::TomlField {
            root,
            path,
            section,
            key,
        } => toml_field(&read_root(roots, root, path)?, section, key)
            .ok_or_else(|| format!("fact `{}` found no `{section}.{key}`", fact.key))?,
        FactSource::Regex {
            root,
            path,
            pattern,
        } => {
            let text = read_root(roots, root, path)?;
            Regex::new(pattern)
                .map_err(|error| error.to_string())?
                .captures(&text)
                .and_then(|captures| captures.get(1))
                .map(|capture| capture.as_str().to_owned())
                .ok_or_else(|| format!("fact `{}` regex did not capture a value", fact.key))?
        }
    };
    if value.len() > 256 || value.contains('\n') || value.contains('\r') {
        return Err(format!("fact `{}` is not one bounded line", fact.key));
    }
    Ok(ResolvedFact {
        key: fact.key.clone(),
        value,
        expose_to_model: fact.expose_to_model,
    })
}

fn read_root(
    roots: &BTreeMap<&str, &ResolvedRoot>,
    root: &str,
    path: &str,
) -> Result<String, String> {
    let root = roots
        .get(root)
        .ok_or_else(|| format!("unknown root `{root}`"))?;
    let file = root.host.join(path);
    std::fs::read_to_string(&file).map_err(|error| format!("reading `{}`: {error}", file.display()))
}

fn toml_field(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == format!("[{section}]");
        } else if inside
            && let Some(value) = trimmed.strip_prefix(&format!("{key} = \""))
            && let Some(value) = value.strip_suffix('"')
        {
            return Some(value.to_owned());
        }
    }
    None
}

#[derive(Debug)]
struct ResolvedValues {
    items: Vec<String>,
    bound: Option<String>,
}

fn resolve_values(workspace: &Path, spec: &ValueSpec) -> Result<ResolvedValues, String> {
    match &spec.source {
        ValueSource::JsonMapKeys { path, pointer } => {
            let file = contained_existing(workspace, Path::new(path))?;
            let value: Value = serde_json::from_str(
                &std::fs::read_to_string(&file)
                    .map_err(|error| format!("reading `{}`: {error}", file.display()))?,
            )
            .map_err(|error| format!("parsing `{}`: {error}", file.display()))?;
            let map = value
                .pointer(pointer)
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    format!("`{}` pointer `{pointer}` is not an object", file.display())
                })?;
            if map.len() > MAX_LIST_ITEMS {
                return Err(format!(
                    "`{}` pointer `{pointer}` has {} entries; the bound is {MAX_LIST_ITEMS}",
                    file.display(),
                    map.len()
                ));
            }
            let mut items = Vec::new();
            for (name, value) in map {
                if name.len() > 256 || name.contains(['\n', '\r']) {
                    return Err(format!(
                        "`{}` entry name is not one bounded line",
                        file.display()
                    ));
                }
                if !value.is_string() {
                    return Err(format!(
                        "`{}` entry `{name}` is not a string",
                        file.display()
                    ));
                }
                items.push(name.clone());
            }
            items.sort();
            Ok(ResolvedValues { items, bound: None })
        }
        ValueSource::TaskfileTasks { files } => resolve_taskfiles(workspace, files),
    }
}

fn contained_existing(workspace: &Path, relative: &Path) -> Result<PathBuf, String> {
    validate_relative(&relative.display().to_string())?;
    let canonical = workspace.join(relative).canonicalize().map_err(|error| {
        format!(
            "project file `{}`: {error}",
            workspace.join(relative).display()
        )
    })?;
    if !canonical.starts_with(workspace) {
        return Err(format!(
            "project file `{}` escapes the workspace",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn resolve_taskfiles(workspace: &Path, candidates: &[String]) -> Result<ResolvedValues, String> {
    let present: Vec<&String> = candidates
        .iter()
        .filter(|path| workspace.join(path).is_file())
        .collect();
    if present.len() != 1 {
        return Err(format!(
            "Taskfile discovery requires exactly one of {}; found {}",
            candidates.join(", "),
            present.len()
        ));
    }
    let root = PathBuf::from(present[0]);
    let mut tasks = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    collect_taskfile(workspace, &root, "", &mut visiting, &mut tasks)?;
    Ok(ResolvedValues {
        items: tasks.into_iter().collect(),
        bound: Some(root.display().to_string()),
    })
}

fn collect_taskfile(
    workspace: &Path,
    relative: &Path,
    prefix: &str,
    visiting: &mut BTreeSet<PathBuf>,
    tasks: &mut BTreeSet<String>,
) -> Result<(), String> {
    let canonical = contained_existing(workspace, relative)?;
    if !visiting.insert(canonical.clone()) {
        return Err(format!(
            "Taskfile include cycle reaches `{}`",
            canonical.display()
        ));
    }
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(
        &std::fs::read_to_string(&canonical)
            .map_err(|error| format!("reading `{}`: {error}", canonical.display()))?,
    )
    .map_err(|error| format!("parsing `{}`: {error}", canonical.display()))?;
    let map = document
        .as_mapping()
        .ok_or_else(|| format!("`{}` is not a mapping", canonical.display()))?;
    if let Some(task_map) = yaml_mapping(map, "tasks")? {
        for (key, definition) in task_map {
            let name = key
                .as_str()
                .ok_or_else(|| format!("`{}` has a non-string task name", canonical.display()))?;
            let internal = definition
                .as_mapping()
                .map(|map| yaml_bool(map, "internal"))
                .transpose()?
                .flatten()
                .unwrap_or(false);
            if internal {
                continue;
            }
            let full = format!("{prefix}{name}");
            if full.len() > 256 || full.contains(['\n', '\r']) || full.contains("{{") {
                return Err(format!(
                    "Taskfile task `{full}` is not one bounded static name"
                ));
            }
            if !tasks.insert(full.clone()) {
                return Err(format!("Taskfile task `{full}` is declared more than once"));
            }
            if tasks.len() > MAX_LIST_ITEMS {
                return Err(format!(
                    "Taskfile exposes more than {MAX_LIST_ITEMS} public tasks"
                ));
            }
        }
    }
    if let Some(includes) = yaml_mapping(map, "includes")? {
        for (key, definition) in includes {
            let name = key.as_str().ok_or_else(|| {
                format!("`{}` has a non-string include name", canonical.display())
            })?;
            let (taskfile, directory, flatten, internal) = parse_include(definition)?;
            if internal {
                continue;
            }
            if taskfile.contains("{{") || directory.as_deref().is_some_and(|dir| dir.contains("{{"))
            {
                return Err(format!("Taskfile include `{name}` uses a template"));
            }
            if taskfile.contains("://") {
                return Err(format!("Taskfile include `{name}` is remote"));
            }
            let parent = relative.parent().unwrap_or_else(|| Path::new(""));
            // `dir` is the included tasks' working directory; it does not re-base the Taskfile
            // path. Task resolves `taskfile` from the including document.
            let include = parent.join(taskfile);
            let child_prefix = if flatten {
                prefix.to_owned()
            } else {
                format!("{prefix}{name}:")
            };
            collect_taskfile(workspace, &include, &child_prefix, visiting, tasks)?;
        }
    }
    visiting.remove(&canonical);
    Ok(())
}

fn yaml_mapping<'a>(
    map: &'a serde_yaml_ng::Mapping,
    key: &str,
) -> Result<Option<&'a serde_yaml_ng::Mapping>, String> {
    map.get(serde_yaml_ng::Value::String(key.to_owned()))
        .map(|value| {
            value
                .as_mapping()
                .ok_or_else(|| format!("`{key}` is not a mapping"))
        })
        .transpose()
}

fn yaml_bool(map: &serde_yaml_ng::Mapping, key: &str) -> Result<Option<bool>, String> {
    map.get(serde_yaml_ng::Value::String(key.to_owned()))
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| format!("`{key}` is not a boolean"))
        })
        .transpose()
}

fn parse_include(
    value: &serde_yaml_ng::Value,
) -> Result<(String, Option<String>, bool, bool), String> {
    if let Some(path) = value.as_str() {
        return Ok((path.to_owned(), None, false, false));
    }
    let map = value
        .as_mapping()
        .ok_or_else(|| "Taskfile include is neither a path nor an object".to_owned())?;
    let string = |key: &str| {
        map.get(serde_yaml_ng::Value::String(key.to_owned()))
            .map(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| format!("include `{key}` is not a string"))
            })
            .transpose()
    };
    let taskfile = string("taskfile")?
        .ok_or_else(|| "Taskfile include object names no `taskfile`".to_owned())?;
    Ok((
        taskfile,
        string("dir")?,
        yaml_bool(map, "flatten")?.unwrap_or(false),
        yaml_bool(map, "internal")?.unwrap_or(false),
    ))
}

fn resolve_tool(
    spec: &ToolSpec,
    values: &BTreeMap<String, Vec<String>>,
    bound_values: &BTreeMap<String, String>,
) -> ResolvedTool {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, parameter) in &spec.parameters {
        let mut schema = match parameter.kind {
            ParameterKind::String | ParameterKind::WorkspacePath => json!({"type":"string"}),
            ParameterKind::Boolean => json!({"type":"boolean"}),
            ParameterKind::StringList | ParameterKind::WorkspacePathList => {
                json!({"type":"array","items":{"type":"string"},"maxItems":parameter.max_items.unwrap_or(MAX_LIST_ITEMS)})
            }
            ParameterKind::Enum => json!({
                "type":"string",
                "enum": values.get(parameter.values.as_deref().unwrap_or_default()).cloned().unwrap_or_default(),
            }),
        };
        if let Some(description) = &parameter.description {
            schema["description"] = Value::String(description.clone());
        }
        if let Some(default) = &parameter.default {
            schema["default"] = default.clone();
        }
        if let Some(minimum) = parameter.min_items {
            schema["minItems"] = json!(minimum);
        }
        properties.insert(name.clone(), schema);
        if parameter.required {
            required.push(name.clone());
        }
    }
    let mut schema = json!({
        "type":"object",
        "properties": properties,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    ResolvedTool {
        name: spec.name.clone(),
        description: spec.description.clone(),
        role: spec.role,
        input_schema: schema,
        parameters: spec.parameters.clone(),
        commands: spec.commands.clone(),
        writes: spec.writes.clone(),
        empty_stdout: spec.empty_stdout,
        workspace_writes: spec.workspace_writes,
        values: values.clone(),
        bound_values: bound_values.clone(),
    }
}

fn validate_arguments(tool: &ResolvedTool, arguments: &Value) -> Result<(), String> {
    let object = arguments
        .as_object()
        .ok_or_else(|| format!("arguments for `{}` must be an object", tool.name))?;
    for name in object.keys() {
        if !tool.parameters.contains_key(name) {
            return Err(format!("`{name}` is not an argument of `{}`", tool.name));
        }
    }
    for (name, parameter) in &tool.parameters {
        let value = object.get(name).or(parameter.default.as_ref());
        if parameter.required && value.is_none() {
            return Err(format!("`{name}` is required by `{}`", tool.name));
        }
        let Some(value) = value else { continue };
        let valid = match parameter.kind {
            ParameterKind::String | ParameterKind::WorkspacePath => value.is_string(),
            ParameterKind::Boolean => value.is_boolean(),
            ParameterKind::StringList | ParameterKind::WorkspacePathList => {
                value.as_array().is_some_and(|items| {
                    items.iter().all(Value::is_string)
                        && items.len() >= parameter.min_items.unwrap_or(0)
                        && items.len() <= parameter.max_items.unwrap_or(MAX_LIST_ITEMS)
                })
            }
            ParameterKind::Enum => value.as_str().is_some_and(|value| {
                parameter
                    .values
                    .as_ref()
                    .and_then(|name| tool.values.get(name))
                    .is_some_and(|items| items.iter().any(|item| item == value))
            }),
        };
        if !valid {
            return Err(format!("`{name}` is not valid for `{}`", tool.name));
        }
        if matches!(
            parameter.kind,
            ParameterKind::WorkspacePath | ParameterKind::WorkspacePathList
        ) {
            let paths: Vec<&str> = value.as_str().map_or_else(
                || {
                    value
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect()
                },
                |path| vec![path],
            );
            for path in paths {
                validate_relative(path)?;
            }
        }
    }
    Ok(())
}

fn compile_argv(
    command: &CommandSpec,
    arguments: &Value,
    parameters: &BTreeMap<String, ParameterSpec>,
    values: &BTreeMap<String, String>,
    item: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut argv = Vec::new();
    for segment in &command.argv {
        match segment {
            ArgvSegment::Literal(value) => argv.push(value.clone()),
            ArgvSegment::Dynamic(dynamic) => {
                if let Some(name) = &dynamic.arg
                    && let Some(value) = argument(arguments, parameters, name)
                {
                    argv.push(
                        value
                            .as_str()
                            .ok_or_else(|| format!("`{name}` is not a string"))?
                            .to_owned(),
                    );
                } else if let Some(name) = &dynamic.value {
                    argv.push(
                        values
                            .get(name)
                            .ok_or_else(|| format!("resolved value `{name}` is absent"))?
                            .clone(),
                    );
                } else if let Some(option) = &dynamic.option
                    && let Some(value) = argument(arguments, parameters, &option.arg)
                {
                    argv.push(option.name.clone());
                    argv.push(
                        value
                            .as_str()
                            .ok_or_else(|| format!("`{}` is not a string", option.arg))?
                            .to_owned(),
                    );
                } else if let Some(flag) = &dynamic.flag
                    && argument(arguments, parameters, &flag.when).and_then(Value::as_bool)
                        == Some(true)
                {
                    argv.push(flag.name.clone());
                } else if let Some(rest) = &dynamic.rest
                    && let Some(values) =
                        argument(arguments, parameters, &rest.arg).and_then(Value::as_array)
                    && !values.is_empty()
                {
                    if let Some(separator) = &rest.separator {
                        argv.push(separator.clone());
                    }
                    argv.extend(
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned),
                    );
                } else if let Some(name) = &dynamic.each {
                    argv.extend(string_list(arguments, name)?);
                } else if dynamic.item {
                    argv.push(
                        item.ok_or_else(|| "argv item is absent".to_owned())?
                            .to_owned(),
                    );
                }
            }
        }
    }
    Ok(argv)
}

fn argument<'a>(
    arguments: &'a Value,
    parameters: &'a BTreeMap<String, ParameterSpec>,
    name: &str,
) -> Option<&'a Value> {
    arguments.get(name).or_else(|| {
        parameters
            .get(name)
            .and_then(|parameter| parameter.default.as_ref())
    })
}

fn string_list(arguments: &Value, name: &str) -> Result<Vec<String>, String> {
    arguments
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("`{name}` must be an array of strings"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("`{name}` contains a non-string"))
        })
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_one_strict_registry() {
        let registry = Registry::builtins().expect("built-ins");
        assert_eq!(registry.names(), ["go", "npm", "rust", "taskfile", "yarn"]);
    }

    #[test]
    fn a_custom_provider_cannot_replace_a_builtin() {
        let mut registry = Registry::builtins().expect("built-ins");
        let error = registry
            .add_document(BUILTIN_GO, "named.yaml")
            .expect_err("collision");
        assert!(error.contains("collides"));
    }

    #[test]
    fn taskfile_discovery_is_static_and_follows_local_includes() {
        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            directory.path().join("Taskfile.yml"),
            "version: '3'\nincludes:\n  child: child.yml\ntasks:\n  build: {}\n  hidden:\n    internal: true\n",
        )
        .expect("root");
        std::fs::write(
            directory.path().join("child.yml"),
            "version: '3'\ntasks:\n  test: {}\n",
        )
        .expect("child");
        let resolved = resolve_taskfiles(
            directory.path(),
            &["Taskfile.yml".to_owned(), "Taskfile.yaml".to_owned()],
        )
        .expect("tasks");
        assert_eq!(resolved.items, ["build", "child:test"]);
        assert_eq!(resolved.bound.as_deref(), Some("Taskfile.yml"));
    }

    #[test]
    fn dynamic_task_visibility_refuses_instead_of_becoming_public() {
        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            directory.path().join("Taskfile.yml"),
            "version: '3'\ntasks:\n  maybe:\n    internal: '{{.PRIVATE}}'\n",
        )
        .expect("Taskfile");
        let error = resolve_taskfiles(directory.path(), &["Taskfile.yml".to_owned()])
            .expect_err("dynamic visibility refuses");
        assert!(error.contains("not a boolean"), "{error}");
    }

    #[test]
    fn taskfile_run_binds_the_exact_manifest_task_and_argument_separator() {
        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            directory.path().join("Taskfile.yml"),
            "version: '3'\ntasks:\n  build: {}\n",
        )
        .expect("Taskfile");
        let document: Document = serde_yaml_ng::from_str(BUILTIN_TASKFILE).expect("built-in");
        let provider = &document.toolchains[0];
        let source = &provider.values[0];
        let resolved = resolve_values(directory.path(), source).expect("static tasks");
        let values = BTreeMap::from([(source.name.clone(), resolved.items)]);
        let bound = BTreeMap::from([(source.name.clone(), resolved.bound.expect("manifest"))]);
        let tool = resolve_tool(&provider.tools[0], &values, &bound);
        let plan = tool
            .plan(&json!({"task":"build","args":["--verbose"]}))
            .expect("typed call");
        assert_eq!(
            plan.argv,
            [vec![
                "task",
                "--taskfile",
                "Taskfile.yml",
                "--disable-fuzzy",
                "build",
                "--",
                "--verbose",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()]
        );
    }

    #[test]
    fn taskfile_cycles_and_workspace_escapes_refuse_the_whole_discovery() {
        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            directory.path().join("Taskfile.yml"),
            "version: '3'\nincludes:\n  again: Taskfile.yml\ntasks: {}\n",
        )
        .expect("cycle");
        let error = resolve_taskfiles(directory.path(), &["Taskfile.yml".to_owned()])
            .expect_err("cycle refuses");
        assert!(error.contains("cycle"), "{error}");

        std::fs::write(
            directory.path().join("Taskfile.yml"),
            "version: '3'\nincludes:\n  outside: ../outside.yml\ntasks: {}\n",
        )
        .expect("escape");
        let error = resolve_taskfiles(directory.path(), &["Taskfile.yml".to_owned()])
            .expect_err("escape refuses");
        assert!(error.contains("contained relative path"), "{error}");
    }

    #[test]
    fn package_scripts_are_enum_values_and_non_string_entries_refuse() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = ValueSpec {
            name: "scripts".to_owned(),
            source: ValueSource::JsonMapKeys {
                path: "package.json".to_owned(),
                pointer: "/scripts".to_owned(),
            },
        };
        std::fs::write(
            directory.path().join("package.json"),
            r#"{"scripts":{"test":"node test.js","build":"node build.js"}}"#,
        )
        .expect("package");
        let values = resolve_values(directory.path(), &source).expect("scripts");
        assert_eq!(values.items, ["build", "test"]);

        std::fs::write(
            directory.path().join("package.json"),
            r#"{"scripts":{"test":["not","a","script"]}}"#,
        )
        .expect("bad package");
        let error = resolve_values(directory.path(), &source).expect_err("non-string refuses");
        assert!(error.contains("not a string"), "{error}");
    }

    #[test]
    fn conflicting_javascript_lockfiles_refuse_auto_discovery() {
        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            directory.path().join("package.json"),
            r#"{"scripts":{"test":"x"}}"#,
        )
        .expect("package");
        std::fs::write(directory.path().join("package-lock.json"), "{}").expect("npm lock");
        std::fs::write(directory.path().join("yarn.lock"), "lock").expect("yarn lock");
        let error = Registry::builtins()
            .expect("built-ins")
            .resolve(directory.path(), None)
            .expect_err("conflict");
        assert!(error.contains("conflict"), "{error}");
    }
}
