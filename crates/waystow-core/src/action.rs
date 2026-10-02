//! Declarative actions: TOML specs, validation, which actions apply to a
//! selection, and turning an action plus inputs into a runnable [`Plan`].
//!
//! Commands are argv arrays and never pass through a shell. Placeholders:
//! `{in}` (first input), `{ins}` (every input, only as a whole argument),
//! `{out}`, `{dir}`, `{name}`, `{stem}`, `{ext}`, and each param by name.
//! `{{` is a literal `{`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::{mime, naming};

/// The built-in action pack.
pub const BUILTIN: &str = include_str!("../assets/actions.toml");

/// How many inputs an action takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arity {
    /// One job per input.
    #[default]
    Each,
    /// One job over all inputs (at least `min_inputs`).
    All,
    /// Exactly two inputs (diff).
    Pair,
}

/// What happens to the command's standard output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stdout {
    /// Discarded.
    #[default]
    Ignore,
    /// Copied to the clipboard as text.
    Clipboard,
    /// Added to the shelf as a text snippet.
    Shelf,
    /// Written to the output file (for tools that only print, like `jq`).
    File,
}

/// How progress is read from the tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Progress {
    /// Indeterminate.
    #[default]
    None,
    /// ffmpeg: `Duration:` on stderr and `-progress pipe:1` on stdout.
    Ffmpeg,
    /// The last `NN%` on any line of stdout or stderr.
    Percent,
}

/// Actions WayStow performs itself instead of running a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Internal {
    /// Copy the paths (or URL) as text.
    CopyPath,
    /// Copy the files, for pasting into a file manager or chat.
    CopyFile,
    /// Copy the image itself.
    CopyImage,
    /// Copy a text file's or snippet's contents.
    CopyContents,
    /// Show the file in the file manager.
    Reveal,
    /// Open with the default app.
    Open,
    /// Rename the file (param `new_name`).
    Rename,
    /// Move to the Trash.
    Trash,
    /// Save a snippet as a text file.
    SaveText,
}

/// A user-settable parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    /// Placeholder name.
    pub name: String,
    /// Label in the UI; defaults to the name.
    #[serde(default)]
    pub label: String,
    /// Type and default.
    #[serde(flatten)]
    pub kind: ParamKind,
}

/// Parameter types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParamKind {
    /// A whole number in a range.
    Int {
        /// Default value.
        default: i64,
        /// Smallest value.
        min: i64,
        /// Largest value.
        max: i64,
    },
    /// One of a list.
    Choice {
        /// Options.
        choices: Vec<String>,
        /// Default option.
        default: String,
    },
    /// Free text; may use the input placeholders, e.g. `"{name}"`.
    Text {
        /// Default value.
        default: String,
    },
}

/// One action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // independent TOML flags, not a state
pub struct Action {
    /// Unique id, e.g. `image.to-webp`. A user action with a built-in id replaces it.
    pub id: String,
    /// Button title.
    pub title: String,
    /// Icon name.
    #[serde(default = "default_icon")]
    pub icon: String,
    /// Shown in the primary row (otherwise under "More").
    #[serde(default)]
    pub primary: bool,
    /// MIME patterns every input must match.
    #[serde(default = "any_mime")]
    pub mime: Vec<String>,
    /// MIME patterns no input may match.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// How many inputs.
    #[serde(default)]
    pub arity: Arity,
    /// Fewest inputs for `arity = "all"`.
    #[serde(default = "one")]
    pub min_inputs: usize,
    /// Programs that must be on PATH; the action is hidden without them.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Parameters.
    #[serde(default)]
    pub params: Vec<Param>,
    /// The command (exclusive with `internal`).
    #[serde(default)]
    pub command: Vec<String>,
    /// Working directory template.
    #[serde(default)]
    pub cwd: Option<String>,
    /// A built-in behaviour instead of a command.
    #[serde(default)]
    pub internal: Option<Internal>,
    /// Output path template; omit for actions that only print or show.
    #[serde(default)]
    pub output: Option<String>,
    /// The output is a directory (created before the command runs).
    #[serde(default)]
    pub output_dir: bool,
    /// What to do with stdout.
    #[serde(default)]
    pub stdout: Stdout,
    /// Progress format.
    #[serde(default)]
    pub progress: Progress,
    /// A GUI the user may close without saving: no output isn't a failure.
    #[serde(default)]
    pub interactive: bool,
    /// Start the program and don't wait for it (terminals, viewers).
    #[serde(default)]
    pub detach: bool,
    /// Exit codes that mean success (`diff` exits 1 when files differ).
    #[serde(default = "zero")]
    pub success_codes: Vec<i32>,
}

fn zero() -> Vec<i32> {
    vec![0]
}

fn default_icon() -> String {
    "ws-sparkle-symbolic".into()
}

fn any_mime() -> Vec<String> {
    vec!["*".into()]
}

const fn one() -> usize {
    1
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionFile {
    #[serde(default)]
    action: Vec<Action>,
}

/// Parse and validate a TOML file of `[[action]]` tables.
///
/// # Errors
///
/// Bad TOML or an invalid action (named in the error).
pub fn parse(text: &str) -> anyhow::Result<Vec<Action>> {
    let file: ActionFile = toml::from_str(text)?;
    for a in &file.action {
        a.validate().with_context(|| format!("action {:?}", a.id))?;
    }
    Ok(file.action)
}

/// The built-in pack overlaid with user files (same id replaces), in order.
///
/// # Errors
///
/// A user file fails to parse; the error names the file.
pub fn load(user_dir: &Path) -> anyhow::Result<Vec<Action>> {
    let mut actions = parse(BUILTIN).context("built-in actions")?;
    let mut files: Vec<PathBuf> = match std::fs::read_dir(user_dir) {
        Ok(rd) => {
            rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", user_dir.display())),
    };
    files.sort();
    for f in files {
        let text = std::fs::read_to_string(&f).with_context(|| format!("reading {}", f.display()))?;
        for a in parse(&text).with_context(|| format!("in {}", f.display()))? {
            match actions.iter_mut().find(|b| b.id == a.id) {
                Some(slot) => *slot = a,
                None => actions.push(a),
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    for a in &actions {
        ensure!(seen.insert(&a.id), "action id {:?} is defined twice in one file", a.id);
    }
    Ok(actions)
}

impl Action {
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(!self.id.trim().is_empty(), "id is empty");
        ensure!(!self.title.trim().is_empty(), "title is empty");
        match (self.internal.is_some(), self.command.is_empty()) {
            (true, false) => bail!("set either `command` or `internal`, not both"),
            (false, true) => bail!("needs a `command` or an `internal` behaviour"),
            _ => {}
        }
        if self.internal.is_some() {
            ensure!(self.output.is_none(), "internal actions have no `output`");
        }
        if self.output_dir {
            ensure!(self.output.is_some(), "`output_dir` needs an `output`");
        }
        if self.stdout == Stdout::File {
            ensure!(self.output.is_some() && !self.output_dir, "`stdout = \"file\"` needs a file `output`");
        }
        if self.detach {
            ensure!(self.output.is_none() && self.stdout == Stdout::Ignore, "detached actions can't have output");
        }
        ensure!(!self.success_codes.is_empty(), "success_codes is empty");
        ensure!(self.min_inputs >= 1, "min_inputs must be at least 1");
        let mut names = std::collections::HashSet::new();
        for p in &self.params {
            ensure!(RESERVED.iter().all(|r| *r != p.name), "param name {:?} is reserved", p.name);
            ensure!(
                !p.name.is_empty() && p.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "param name {:?} must be letters, digits or _",
                p.name
            );
            ensure!(names.insert(p.name.as_str()), "param {:?} is defined twice", p.name);
            match &p.kind {
                ParamKind::Int { default, min, max } => {
                    ensure!(min <= max && (min..=max).contains(&default), "param {:?}: bad range", p.name);
                }
                ParamKind::Choice { choices, default } => {
                    ensure!(choices.contains(default), "param {:?}: default isn't a choice", p.name);
                }
                ParamKind::Text { .. } => {}
            }
        }
        // Expand every template against dummy values: catches unknown
        // placeholders and a misplaced `{ins}` at load time.
        let dummy = [PathBuf::from("/a/b.c"), PathBuf::from("/a/d.e")];
        let values = self.defaults();
        let ctx = Ctx { inputs: &dummy, out: Some(Path::new("/a/o")), params: &values };
        for arg in &self.command {
            ctx.expand_arg(arg).with_context(|| format!("in command argument {arg:?}"))?;
        }
        for t in self.output.iter().chain(&self.cwd) {
            ctx.expand(t, false).with_context(|| format!("in {t:?}"))?;
        }
        if self.command.iter().any(|a| a.contains("{out}")) {
            ensure!(self.output.is_some(), "the command uses {{out}} but there's no `output`");
        }
        Ok(())
    }

    /// Default param values.
    #[must_use]
    pub fn defaults(&self) -> BTreeMap<String, String> {
        self.params
            .iter()
            .map(|p| {
                let v = match &p.kind {
                    ParamKind::Int { default, .. } => default.to_string(),
                    ParamKind::Choice { default, .. } | ParamKind::Text { default } => default.clone(),
                };
                (p.name.clone(), v)
            })
            .collect()
    }

    /// Does this action apply to items with these MIME types?
    #[must_use]
    pub fn applies_to(&self, mimes: &[&str]) -> bool {
        let count_ok = match self.arity {
            Arity::Each => !mimes.is_empty(),
            Arity::All => mimes.len() >= self.min_inputs,
            Arity::Pair => mimes.len() == 2,
        };
        count_ok && mimes.iter().all(|m| mime::matches_any(&self.mime, m) && !mime::matches_any(&self.exclude, m))
    }

    /// Merge user values over the defaults, checking each.
    ///
    /// # Errors
    ///
    /// Unknown param, out-of-range number, or not one of the choices.
    pub fn resolve_params(&self, given: &BTreeMap<String, String>) -> anyhow::Result<BTreeMap<String, String>> {
        let mut values = self.defaults();
        for (k, v) in given {
            let p = self.params.iter().find(|p| &p.name == k).with_context(|| format!("unknown param {k:?}"))?;
            match &p.kind {
                ParamKind::Int { min, max, .. } => {
                    let n: i64 = v.trim().parse().with_context(|| format!("{k} must be a whole number"))?;
                    ensure!((*min..=*max).contains(&n), "{k} must be between {min} and {max}");
                    values.insert(k.clone(), n.to_string());
                }
                ParamKind::Choice { choices, .. } => {
                    ensure!(choices.contains(v), "{k} must be one of {}", choices.join(", "));
                    values.insert(k.clone(), v.clone());
                }
                ParamKind::Text { .. } => {
                    values.insert(k.clone(), v.clone());
                }
            }
        }
        Ok(values)
    }

    /// Split `inputs` into per-job groups according to the arity.
    #[must_use]
    pub fn group(&self, inputs: &[PathBuf]) -> Vec<Vec<PathBuf>> {
        match self.arity {
            Arity::Each => inputs.iter().map(|p| vec![p.clone()]).collect(),
            Arity::All | Arity::Pair => vec![inputs.to_vec()],
        }
    }

    /// Build the plan for one job.
    ///
    /// `free` decides whether a final output path is unused (the caller checks
    /// the filesystem); `fallback_dir` receives outputs when `writable` says the
    /// input's directory can't be written.
    ///
    /// # Errors
    ///
    /// No inputs, a relative input, or a template that expands to a bad path.
    pub fn plan(
        &self,
        job: i64,
        inputs: &[PathBuf],
        params: &BTreeMap<String, String>,
        writable: impl Fn(&Path) -> bool,
        taken: impl Fn(&Path) -> bool,
        fallback_dir: &Path,
    ) -> anyhow::Result<Plan> {
        ensure!(!inputs.is_empty(), "no inputs");
        for i in inputs {
            // Absolute paths can't start with '-', so no input is ever read as an option.
            ensure!(i.is_absolute(), "input {} isn't an absolute path", i.display());
        }
        let ctx = Ctx { inputs, out: None, params };
        let output = match &self.output {
            None => None,
            Some(t) => {
                let mut wanted = PathBuf::from(ctx.expand(t, false)?);
                ensure!(wanted.is_absolute(), "output {} isn't absolute", wanted.display());
                let name = wanted.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ensure!(naming::is_valid_name(&name), "output name {name:?} isn't valid");
                if wanted.parent().is_none_or(|d| !writable(d)) {
                    wanted = fallback_dir.join(name);
                }
                let final_path = naming::unique(&wanted, taken);
                let temp = naming::temp_sibling(&final_path, job);
                Some(Output { wanted, final_path, temp, is_dir: self.output_dir })
            }
        };
        let ctx = Ctx { inputs, out: output.as_ref().map(|o| o.temp.as_path()), params };
        let mut argv = Vec::new();
        for arg in &self.command {
            argv.extend(ctx.expand_arg(arg)?);
        }
        let cwd = self.cwd.as_ref().map(|t| ctx.expand(t, false).map(PathBuf::from)).transpose()?;
        Ok(Plan {
            argv,
            cwd,
            inputs: inputs.to_vec(),
            output,
            stdout: self.stdout,
            progress: self.progress,
            interactive: self.interactive,
            detach: self.detach,
            success_codes: self.success_codes.clone(),
        })
    }
}

/// Placeholders that params can't shadow.
const RESERVED: [&str; 7] = ["in", "ins", "out", "dir", "name", "stem", "ext"];

/// A job's output: the tool writes `temp`, which is renamed to `final_path`
/// only after success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    /// The name the template asked for; numbered variants derive from it.
    pub wanted: PathBuf,
    /// Where the result ends up.
    pub final_path: PathBuf,
    /// Where the tool writes.
    pub temp: PathBuf,
    /// A directory, created before the command runs.
    pub is_dir: bool,
}

/// Everything needed to run one job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Program and arguments.
    pub argv: Vec<String>,
    /// Working directory.
    pub cwd: Option<PathBuf>,
    /// Input files.
    pub inputs: Vec<PathBuf>,
    /// The output, if the action writes one.
    pub output: Option<Output>,
    /// What to do with stdout.
    pub stdout: Stdout,
    /// Progress format.
    pub progress: Progress,
    /// No output isn't a failure.
    pub interactive: bool,
    /// Don't wait for the program.
    pub detach: bool,
    /// Exit codes that mean success.
    pub success_codes: Vec<i32>,
}

struct Ctx<'a> {
    inputs: &'a [PathBuf],
    out: Option<&'a Path>,
    params: &'a BTreeMap<String, String>,
}

impl Ctx<'_> {
    fn expand_arg(&self, arg: &str) -> anyhow::Result<Vec<String>> {
        if arg == "{ins}" {
            return Ok(self.inputs.iter().map(|p| p.to_string_lossy().into_owned()).collect());
        }
        Ok(vec![self.expand(arg, true)?])
    }

    #[allow(clippy::literal_string_with_formatting_args)] // `{out}` is our placeholder syntax
    fn expand(&self, template: &str, in_command: bool) -> anyhow::Result<String> {
        let first = &self.inputs[0];
        let name = first.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (stem, ext) = naming::split_name(&name);
        let mut out = String::new();
        let mut rest = template;
        while let Some(i) = rest.find(['{', '}']) {
            out.push_str(&rest[..i]);
            let tail = &rest[i..];
            if let Some(t) = tail.strip_prefix("{{") {
                out.push('{');
                rest = t;
                continue;
            }
            if let Some(t) = tail.strip_prefix("}}") {
                out.push('}');
                rest = t;
                continue;
            }
            ensure!(tail.starts_with('{'), "unmatched '}}' (write '}}}}' for a literal one)");
            let end = tail.find('}').context("unclosed '{'")?;
            let key = &tail[1..end];
            let value = match key {
                "in" => first.to_string_lossy().into_owned(),
                "ins" => bail!("{{ins}} must be a whole command argument on its own"),
                "out" if in_command => {
                    self.out.context("{out} used but the action has no output")?.to_string_lossy().into_owned()
                }
                "out" => bail!("{{out}} can only be used in the command"),
                "dir" => first.parent().unwrap_or_else(|| Path::new("/")).to_string_lossy().into_owned(),
                "name" => name.clone(),
                "stem" => stem.to_owned(),
                "ext" => ext.to_owned(),
                k => self.params.get(k).with_context(|| format!("unknown placeholder {{{k}}}"))?.clone(),
            };
            out.push_str(&value);
            rest = &tail[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }
}

/// Is `program` on `PATH` (or an executable path)?
#[must_use]
pub fn on_path(program: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let exec = |p: &Path| p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if program.contains('/') {
        return exec(Path::new(program));
    }
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|d| exec(&d.join(program))))
}

#[cfg(test)]
#[allow(clippy::literal_string_with_formatting_args)]
mod tests {
    use super::*;

    fn act(toml: &str) -> Action {
        parse(toml).unwrap().remove(0)
    }

    const WEBP: &str = r#"
[[action]]
id = "image.to-webp"
title = "Convert to WebP"
mime = ["image/*"]
exclude = ["image/webp"]
params = [{ name = "quality", type = "int", default = 82, min = 1, max = 100 }]
command = ["magick", "{in}", "-quality", "{quality}", "{out}"]
output = "{dir}/{stem}.webp"
"#;

    #[test]
    fn builtin_pack_is_valid() {
        let actions = parse(BUILTIN).unwrap();
        assert!(actions.len() >= 25, "{}", actions.len());
        let mut ids: Vec<_> = actions.iter().map(|a| &a.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), actions.len(), "duplicate built-in ids");
    }

    #[test]
    fn plan_expands_and_never_overwrites() {
        let a = act(WEBP);
        let taken = |p: &Path| p == Path::new("/pics/cat.webp");
        let p = a.plan(9, &[PathBuf::from("/pics/cat.png")], &a.defaults(), |_| true, taken, Path::new("/fb")).unwrap();
        let out = p.output.unwrap();
        assert_eq!(out.final_path, PathBuf::from("/pics/cat (1).webp"));
        assert_eq!(out.temp, PathBuf::from("/pics/.cat (1).waystow-9.webp"));
        assert_eq!(p.argv, ["magick", "/pics/cat.png", "-quality", "82", "/pics/.cat (1).waystow-9.webp"]);
    }

    #[test]
    fn unwritable_dir_falls_back() {
        let a = act(WEBP);
        let p = a
            .plan(
                1,
                &[PathBuf::from("/ro/cat.png")],
                &a.defaults(),
                |d| d != Path::new("/ro"),
                |_| false,
                Path::new("/fb"),
            )
            .unwrap();
        assert_eq!(p.output.unwrap().final_path, PathBuf::from("/fb/cat.webp"));
    }

    #[test]
    fn ins_expands_to_many_arguments() {
        let a = act(r#"
[[action]]
id = "pdf.merge"
title = "Merge"
arity = "all"
min_inputs = 2
command = ["qpdf", "--empty", "--pages", "{ins}", "--", "{out}"]
output = "{dir}/{stem}-merged.pdf"
"#);
        let ins = [PathBuf::from("/d/a.pdf"), PathBuf::from("/d/b.pdf")];
        let p = a.plan(1, &ins, &a.defaults(), |_| true, |_| false, Path::new("/fb")).unwrap();
        assert_eq!(p.argv[3..5], ["/d/a.pdf", "/d/b.pdf"]);
        assert!(!a.applies_to(&["application/pdf"]));
        assert!(a.applies_to(&["application/pdf", "application/pdf"]));
    }

    #[test]
    fn bad_templates_fail_at_load() {
        let bad = |cmd: &str| parse(&format!("[[action]]\nid='x'\ntitle='X'\ncommand={cmd}\n")).is_err();
        assert!(bad(r#"["echo", "{nope}"]"#));
        assert!(bad(r#"["echo", "-f{ins}"]"#));
        assert!(bad(r#"["echo", "{out}"]"#), "{{out}} without an output");
        assert!(bad(r#"["echo", "{in"]"#));
        assert!(!bad(r#"["echo", "{{literal}}", "{in}"]"#));
        assert!(parse("[[action]]\nid='x'\ntitle='X'\n").is_err(), "no command");
        assert!(parse("[[action]]\nid='x'\ntitle='X'\ncommand=['a']\ninternal='open'\n").is_err());
        assert!(parse("[[action]]\nid='x'\ntitle='X'\ncommand=['a']\nbogus=1\n").is_err());
    }

    #[test]
    fn literal_braces() {
        let a = act("[[action]]\nid='x'\ntitle='X'\ncommand=['jq', '{{}}', '{in}']\n");
        let p = a.plan(1, &[PathBuf::from("/a.json")], &a.defaults(), |_| true, |_| false, Path::new("/")).unwrap();
        assert_eq!(p.argv, ["jq", "{}", "/a.json"]);
    }

    #[test]
    fn mime_filters() {
        let a = act(WEBP);
        assert!(a.applies_to(&["image/png", "image/jpeg"]));
        assert!(!a.applies_to(&["image/webp"]));
        assert!(!a.applies_to(&["image/png", "application/pdf"]));
        assert!(!a.applies_to(&[]));
    }

    #[test]
    fn params_are_checked() {
        let a = act(WEBP);
        let given = |v: &str| BTreeMap::from([("quality".to_string(), v.to_string())]);
        assert_eq!(a.resolve_params(&given("50")).unwrap()["quality"], "50");
        assert!(a.resolve_params(&given("500")).is_err());
        assert!(a.resolve_params(&given("x")).is_err());
        assert!(a.resolve_params(&BTreeMap::from([("nope".into(), "1".into())])).is_err());
    }

    #[test]
    fn relative_inputs_are_refused() {
        let a = act(WEBP);
        assert!(a.plan(1, &[PathBuf::from("-rf.png")], &a.defaults(), |_| true, |_| false, Path::new("/")).is_err());
    }

    #[test]
    fn user_actions_override_builtins() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mine.toml"), WEBP.replace("Convert to WebP", "To WebP")).unwrap();
        let all = load(dir.path()).unwrap();
        let webp: Vec<_> = all.iter().filter(|a| a.id == "image.to-webp").collect();
        assert_eq!(webp.len(), 1);
        assert_eq!(webp[0].title, "To WebP");
        std::fs::write(dir.path().join("bad.toml"), "[[action]]\nid='y'\n").unwrap();
        let e = load(dir.path()).unwrap_err();
        assert!(format!("{e:#}").contains("bad.toml"), "{e:#}");
    }
}
